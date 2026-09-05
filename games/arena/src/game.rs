//! Replace this module with your game. Hosts depend only on the functions below.
mod save;
pub(crate) use save::export_save_world;
pub use save::{MAX_SAVE_BYTES, export_save, load_save};

use serde::Deserialize;
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::{Path, PathBuf},
};
use titan::input::{ActionValue, BufferedButtons, InputFrame, InputTracker};
use titan::inspection::{InspectionConfig, Inspector};
use titan::render::{
    Color, Image, ImageAssets, ImageId, RenderFrame, SoftwareRenderer, SpriteDraw,
};
use titan::ui::{
    BitmapFont, UiButton, UiNode, UiPointer, UiPointerResult, UiText, append_ui,
    register_ui_inspection,
};
use titan::{App, Component, FixedTime, FixedUpdate, Inspect, Name, Res, ResMut, Startup, World};
use titan_protocol::{
    CaptureResult, CommandMetadata, ErrorCode, FieldMetadata, InputValue, ProtocolError,
};

pub const WIDTH: i32 = 160;
pub const HEIGHT: i32 = 112;
const DOT_SIZE: i32 = 7;
pub const SEED: u32 = 0xA2E4;
pub const SURVIVAL_TICKS: u32 = 1200;
pub const DASH_TICKS: u32 = 6;
pub const DASH_COOLDOWN_TICKS: u32 = 120;
const DASH_SPEED: i32 = 4;
#[derive(Component, Inspect, Clone, Copy)]
struct Enemy {
    /// Whether this pooled enemy is active in the arena
    #[inspect]
    active: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Won,
    Lost,
}
#[derive(Clone, Copy)]
pub struct Run {
    pub elapsed: u32,
    pub health: u32,
    pub spawned: u32,
    pub outcome: Outcome,
    cooldown: u32,
    dash_remaining: u32,
    dash_cooldown: u32,
    facing: (i32, i32),
    dash_direction: (i32, i32),
    random: u32,
}
impl Default for Run {
    fn default() -> Self {
        Self {
            elapsed: 0,
            health: 3,
            spawned: 0,
            outcome: Outcome::Running,
            cooldown: 0,
            dash_remaining: 0,
            dash_cooldown: 0,
            facing: (1, 0),
            dash_direction: (1, 0),
            random: SEED,
        }
    }
}

#[derive(Component, Clone, Copy)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}
#[derive(Component)]
struct Player;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Dash,
}
#[derive(Default)]
struct ScheduledInput {
    enabled: bool,
    frames: BTreeMap<u64, Vec<(Action, ActionValue)>>,
    tracker: InputTracker<Action>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartArgs {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerArgs {
    x: Option<i32>,
    y: Option<i32>,
    pressed: bool,
}
#[derive(Component, Clone, Copy)]
enum HudKind {
    Status,
    Restart,
    Dash,
}
#[derive(Component)]
struct RestartButton;
/// Increments whenever restart or load replaces gameplay and resets host input.
#[derive(Default)]
struct RestartEpoch(u64);
#[derive(Default)]
struct UiPointers {
    local: UiPointer,
    controlled: UiPointer,
}
struct Art {
    player: ImageId,
    enemy: ImageId,
    floor: ImageId,
}

pub fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, apply_scheduled_input);
    app.add_systems(FixedUpdate, crate::live::record_consumed);
    app.add_systems(FixedUpdate, simulate);
    app.add_systems(FixedUpdate, sync_hud);
    app.add_systems(FixedUpdate, crate::live::finish_recording);
    app.add_extractor(render_frame);
    app
}

/// Reset game state without replacing the host clock or inspector request history.
pub fn restart(app: &mut App) {
    app.update_schedule(Startup);
    let players: Vec<_> = app
        .world()
        .iter::<Player>()
        .map(|(entity, _)| entity)
        .collect();
    for entity in players {
        *app.world_mut().get_mut::<Position>(entity).unwrap() = initial_position();
    }
    let enemies: Vec<_> = app.world().iter::<Enemy>().map(|(e, _)| e).collect();
    for e in enemies {
        app.world_mut().get_mut::<Enemy>(e).unwrap().active = false;
    }
    app.world_mut().insert_resource(Run::default());
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    crate::live::begin_recording(app.world_mut());
    let epoch = app.world_mut().resource_mut::<RestartEpoch>().unwrap();
    epoch.0 = epoch.0.wrapping_add(1);
    cancel_ui_pointer(app);
    sync_hud(app.world_mut());
    app.refresh_extracted();
}

pub(crate) fn restart_epoch(app: &App) -> u64 {
    app.world()
        .resource::<RestartEpoch>()
        .map_or(0, |epoch| epoch.0)
}

pub fn cancel_ui_pointer(app: &mut App) {
    if let Some(pointers) = app.world_mut().resource_mut::<UiPointers>() {
        pointers.local.cancel();
        pointers.controlled.cancel();
    }
}

/// The same entity hit test serves local pointer input and controlled commands.
pub fn handle_ui_pointer(
    app: &mut App,
    position: Option<(i32, i32)>,
    pressed: bool,
) -> UiPointerResult {
    update_ui_pointer(app, position, pressed, false)
}

fn update_ui_pointer(
    app: &mut App,
    position: Option<(i32, i32)>,
    pressed: bool,
    controlled: bool,
) -> UiPointerResult {
    app.update_schedule(Startup);
    let mut pointers = app
        .world_mut()
        .remove_resource::<UiPointers>()
        .unwrap_or_default();
    let pointer = if controlled {
        &mut pointers.controlled
    } else {
        &mut pointers.local
    };
    let result = pointer.update(app.world(), position, pressed);
    app.world_mut().insert_resource(pointers);
    if result
        .activated
        .is_some_and(|entity| app.world().get::<RestartButton>(entity).is_some())
    {
        restart(app);
    }
    result
}

pub(crate) fn clear_scheduled_input(app: &mut App) {
    app.world_mut().insert_resource(ScheduledInput::default());
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
}

fn initial_position() -> Position {
    Position {
        x: WIDTH / 2,
        y: (HEIGHT + 18) / 2,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn configured_inspector(output_path: PathBuf, config: InspectionConfig) -> Inspector {
    inspector_with_capture(config, move |app| {
        let image = render_image(app.world())?;
        write_ppm(&output_path, &image).map_err(|error| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("capture write failed: {error}"),
            )
        })?;
        Ok(CaptureResult {
            width: image.width(),
            height: image.height(),
            format: "ppm".into(),
            artifact: output_path.to_string_lossy().into_owned(),
            checksum: format!("{:016x}", image_checksum(&image)),
        })
    })
}

pub fn inspector_with_capture(
    config: InspectionConfig,
    capture: impl FnMut(&App) -> Result<CaptureResult, ProtocolError> + Send + 'static,
) -> Inspector {
    let mut inspector = Inspector::new(config);
    register_ui_inspection(&mut inspector).expect("unique UI fields");
    inspector
        .register_inspectable::<Enemy>()
        .expect("unique enemy fields");
    for (field, maximum) in [("x", WIDTH - DOT_SIZE), ("y", HEIGHT - DOT_SIZE)] {
        inspector
            .register_field::<Position, i32>(
                field,
                FieldMetadata {
                    type_name: "i32".into(),
                    description: "Arena sprite pixel coordinate".into(),
                    writable: true,
                    minimum: Some(if field == "y" { 18.0 } else { 0.0 }),
                    maximum: Some(f64::from(maximum)),
                    unit: Some("pixel".into()),
                },
                move |position| if field == "x" { position.x } else { position.y },
                |_, _| Ok(()),
                move |position, value| {
                    if field == "x" {
                        position.x = value;
                    } else {
                        position.y = value;
                    }
                },
            )
            .expect("unique position field");
    }

    inspector
        .register_command(
            CommandMetadata {
                name: "restart".into(),
                description: "Reset the scene and pending input; the host frame stays monotonic."
                    .into(),
                arguments: BTreeMap::new(),
            },
            |app, _: RestartArgs| {
                restart(app);
                Ok(())
            },
        )
        .expect("unique restart command");
    inspector
        .register_command(
            CommandMetadata {
                name: "ui_pointer".into(),
                description: "Update the primary UI pointer; press and release inside ui/restart activates the same in-game button. Omit both coordinates to release outside.".into(),
                arguments: [
                    ("x".into(), FieldMetadata { type_name: "Option<i32>".into(), description: "Framebuffer pixel x, or null with y".into(), writable: false, minimum: None, maximum: None, unit: Some("pixel".into()) }),
                    ("y".into(), FieldMetadata { type_name: "Option<i32>".into(), description: "Framebuffer pixel y, or null with x".into(), writable: false, minimum: None, maximum: None, unit: Some("pixel".into()) }),
                    ("pressed".into(), FieldMetadata { type_name: "bool".into(), description: "Whether the primary pointer is pressed".into(), writable: false, minimum: None, maximum: None, unit: None }),
                ].into(),
            },
            |app, args: PointerArgs| {
                let position = match (args.x, args.y) {
                    (Some(x), Some(y)) => Some((x, y)),
                    (None, None) => None,
                    _ => return Err(ProtocolError::new(ErrorCode::InvalidValue, "provide both pointer coordinates or neither")),
                };
                update_ui_pointer(app, position, args.pressed, true);
                Ok(())
            },
        )
        .expect("unique pointer command");
    inspector
        .register_command(
            CommandMetadata {
                name: "verify_survival".into(),
                description: "Check the completed run survived; failures retain diagnostic state."
                    .into(),
                arguments: BTreeMap::new(),
            },
            |app, _: RestartArgs| {
                if app
                    .world()
                    .resource::<Run>()
                    .is_some_and(|r| r.outcome == Outcome::Won)
                {
                    Ok(())
                } else {
                    Err(ProtocolError::new(
                        ErrorCode::InvalidValue,
                        "run has not survived; inspect elapsed, health and recent inputs",
                    ))
                }
            },
        )
        .expect("unique verification command");
    inspector.register_input_handler(|app, frame, actions| {
        if frame <= app.world().resource::<FixedTime>().unwrap().tick() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidValue,
                "input frame must be in the future",
            ));
        }
        let values = actions
            .iter()
            .map(|(name, value)| {
                let action = match name.as_str() {
                    "up" => Action::Up,
                    "down" => Action::Down,
                    "left" => Action::Left,
                    "right" => Action::Right,
                    "dash" => Action::Dash,
                    _ => {
                        return Err(ProtocolError::new(
                            ErrorCode::InvalidValue,
                            format!("unknown action: {name}"),
                        ));
                    }
                };
                let value = match value {
                    InputValue::Button(true) => ActionValue::PRESSED,
                    InputValue::Button(false) => ActionValue::RELEASED,
                    InputValue::Axis(_) => {
                        return Err(ProtocolError::new(
                            ErrorCode::InvalidValue,
                            "arena actions require button values",
                        ));
                    }
                };
                Ok((action, value))
            })
            .collect::<Result<Vec<_>, ProtocolError>>()?;
        let scheduled = app.world_mut().resource_mut::<ScheduledInput>().unwrap();
        scheduled.enabled = true;
        scheduled.frames.insert(frame, values);
        Ok(())
    });
    crate::live::register_queries(&mut inspector);
    inspector.register_capture_handler(capture);
    inspector
}

/// Bounded game-specific position values for native diagnostic bundles.
#[cfg(not(target_arch = "wasm32"))]
pub fn diagnostic_positions(world: &World) -> serde_json::Value {
    let positions: Vec<_> = world
        .entities()
        .filter_map(|entity| {
            world.get::<Position>(entity).map(|position| {
                serde_json::json!({
                    "entity": {"index": entity.index(), "generation": entity.generation()},
                    "x": position.x,
                    "y": position.y,
                    "enemy_active": world.get::<Enemy>(entity).map(|enemy| enemy.active),
                })
            })
        })
        .take(1000)
        .collect();
    serde_json::json!({"positions": positions, "run": run_status(world)})
}

pub fn render_image(world: &World) -> Result<Image, ProtocolError> {
    let assets = world.resource::<ImageAssets>().ok_or_else(|| {
        ProtocolError::new(ErrorCode::Busy, "run startup with step before capturing")
    })?;
    SoftwareRenderer::render(&render_frame(world), assets).map_err(|error| {
        ProtocolError::new(ErrorCode::Internal, format!("render failed: {error:?}"))
    })
}

fn apply_scheduled_input(
    time: Res<FixedTime>,
    mut scheduled: ResMut<ScheduledInput>,
    mut input: ResMut<InputFrame<Action>>,
) {
    if scheduled.enabled {
        // Each submitted map is a complete snapshot for one completed frame.
        let values = scheduled
            .frames
            .remove(&(time.tick() + 1))
            .unwrap_or_default();
        *input = scheduled.tracker.sample(values);
    }
}

fn setup(world: &mut World) {
    let mut assets = ImageAssets::new();
    let player = assets.insert(
        Image::from_fn(7, 7, |x, y| {
            if (x == 2 || x == 4) && y == 2 {
                Color::rgb(255, 255, 255)
            } else if x == 0 || y == 0 || x == 6 || y == 6 {
                Color::rgb(20, 100, 120)
            } else {
                Color::rgb(80, 230, 220)
            }
        })
        .unwrap(),
    );
    let enemy = assets.insert(
        Image::from_fn(7, 7, |x, y| {
            if (x == 2 || x == 4) && y == 2 {
                Color::rgb(255, 230, 150)
            } else if x == 0 || y == 0 || x == 6 || y == 6 {
                Color::rgb(110, 30, 50)
            } else {
                Color::rgb(240, 80, 100)
            }
        })
        .unwrap(),
    );
    let floor = assets.insert(
        Image::from_fn(WIDTH as u32, HEIGHT as u32, |x, y| {
            if y < 17 {
                Color::rgb(12, 18, 30)
            } else if x == 0 || x == 159 || y == 17 || y == 111 {
                Color::rgb(85, 110, 145)
            } else if x % 16 == 0 || y % 16 == 0 {
                Color::rgb(31, 43, 61)
            } else {
                Color::rgb(24, 33, 48)
            }
        })
        .unwrap(),
    );
    let font = BitmapFont::tiny(&mut assets);
    world.insert_resource(font);
    world.insert_resource(assets);
    world.insert_resource(Art {
        player,
        enemy,
        floor,
    });
    world.insert_resource(Run::default());
    world.insert_resource(RestartEpoch::default());
    world.insert_resource(UiPointers::default());
    world.spawn_with((initial_position(), Player, Name::new("player")));
    for index in 0..14 {
        world.spawn_with((
            Position { x: 0, y: 18 },
            Enemy { active: false },
            Name::new(format!("enemy-{index}")),
        ));
    }
    let color = Color::rgb(225, 239, 249);
    world.spawn_with((
        HudKind::Status,
        UiNode::new(4, 3, 76, 5),
        UiText::new("").with_color(color),
        Name::new("ui/status"),
    ));
    world.spawn_with((
        HudKind::Restart,
        UiNode::new(4, 10, 36, 5),
        UiText::new("").with_color(color),
        UiButton::default(),
        RestartButton,
        Name::new("ui/restart"),
    ));
    world.spawn_with((
        HudKind::Dash,
        UiNode::new(112, 10, 40, 5),
        UiText::new("").with_color(color),
        Name::new("ui/dash"),
    ));
    sync_hud(world);
    crate::live::begin_recording(world);
    crate::live::finish_recording(world);
}

fn sync_hud(world: &mut World) {
    let run = *world.resource::<Run>().unwrap();
    let entities: Vec<_> = world
        .iter::<HudKind>()
        .map(|(entity, kind)| (entity, *kind))
        .collect();
    for (entity, kind) in entities {
        let text = match kind {
            HudKind::Status => format!("HP {}   TIME {:02}/20", run.health, run.elapsed / 60),
            HudKind::Restart => match run.outcome {
                Outcome::Running => "R RESTART",
                Outcome::Won => "WON R RESTART",
                Outcome::Lost => "LOST R RESTART",
            }
            .to_owned(),
            HudKind::Dash if run.dash_cooldown == 0 => "DASH READY".into(),
            HudKind::Dash => {
                let tenths = run.dash_cooldown.div_ceil(6);
                format!("DASH {}.{}S", tenths / 10, tenths % 10)
            }
        };
        world.get_mut::<UiNode>(entity).unwrap().width = text.chars().count() as u32 * 4;
        world.get_mut::<UiText>(entity).unwrap().text = text;
    }
}

fn simulate(world: &mut World) {
    let mut run = *world.resource::<Run>().unwrap();
    if run.outcome != Outcome::Running {
        return;
    }
    run.elapsed += 1;
    run.cooldown = run.cooldown.saturating_sub(1);
    let input = world.resource::<InputFrame<Action>>().unwrap();
    let dx = i32::from(input.is_active(&Action::Right)) - i32::from(input.is_active(&Action::Left));
    let dy = i32::from(input.is_active(&Action::Down)) - i32::from(input.is_active(&Action::Up));
    run.dash_cooldown = run.dash_cooldown.saturating_sub(1);
    if dx != 0 || dy != 0 {
        run.facing = (dx, dy);
    }
    // Cooldown counts from the activation tick. A held button never queues
    // another dash, and movement cannot steer an already active dash.
    if input.just_pressed(&Action::Dash) && run.dash_cooldown == 0 {
        run.dash_direction = run.facing;
        run.dash_remaining = DASH_TICKS;
        run.dash_cooldown = DASH_COOLDOWN_TICKS;
    }
    let (dx, dy) = if run.dash_remaining > 0 {
        run.dash_remaining -= 1;
        (
            run.dash_direction.0 * DASH_SPEED,
            run.dash_direction.1 * DASH_SPEED,
        )
    } else {
        (dx, dy)
    };
    let player = world.iter::<Player>().next().unwrap().0;
    let position = world.get_mut::<Position>(player).unwrap();
    position.x = (position.x + dx).clamp(0, WIDTH - DOT_SIZE);
    position.y = (position.y + dy).clamp(18, HEIGHT - DOT_SIZE);
    let target = *position;
    let enemies: Vec<_> = world.iter::<Enemy>().map(|(e, v)| (e, *v)).collect();
    if run.elapsed % 240 == 1
        && let Some((entity, _)) = enemies.iter().find(|(_, e)| !e.active)
    {
        run.random = run.random.wrapping_mul(1664525).wrapping_add(1013904223);
        let edge = run.random % 4;
        let offset = (run.random >> 8) as i32;
        *world.get_mut::<Position>(*entity).unwrap() = match edge {
            0 => Position {
                x: 0,
                y: 18 + offset % 88,
            },
            1 => Position {
                x: 153,
                y: 18 + offset % 88,
            },
            2 => Position {
                x: offset % 154,
                y: 18,
            },
            _ => Position {
                x: offset % 154,
                y: 105,
            },
        };
        world.get_mut::<Enemy>(*entity).unwrap().active = true;
        run.spawned += 1;
    }
    for (entity, _) in enemies {
        if !world.get::<Enemy>(entity).unwrap().active {
            continue;
        }
        let p = world.get_mut::<Position>(entity).unwrap();
        if run.elapsed.is_multiple_of(5) {
            p.x += (target.x - p.x).signum();
            p.y += (target.y - p.y).signum();
        }
        if (target.x - p.x).abs() < DOT_SIZE
            && (target.y - p.y).abs() < DOT_SIZE
            && run.cooldown == 0
        {
            run.health = run.health.saturating_sub(1);
            run.cooldown = 60;
        }
    }
    if run.health == 0 {
        run.outcome = Outcome::Lost;
    } else if run.elapsed >= SURVIVAL_TICKS {
        run.outcome = Outcome::Won;
    }
    world.insert_resource(run);
}
fn render_frame(world: &World) -> RenderFrame {
    let mut frame = RenderFrame::new(WIDTH as u32, HEIGHT as u32, Color::rgb(24, 30, 44));
    if let Some(art) = world.resource::<Art>() {
        frame.push(SpriteDraw::new(art.floor, 0, 0));
        for (entity, p) in world.iter::<Position>() {
            let sprite = if world.get::<Player>(entity).is_some() {
                Some(art.player)
            } else if world.get::<Enemy>(entity).is_some_and(|e| e.active) {
                Some(art.enemy)
            } else {
                None
            };
            if let Some(sprite) = sprite {
                frame.push(SpriteDraw::new(sprite, p.x, p.y));
            }
        }
        append_ui(world, &mut frame);
    }
    frame
}
fn run_status(world: &World) -> serde_json::Value {
    world
        .resource::<Run>()
        .map(|run| {
            serde_json::json!({
                "seed": SEED,
                "elapsed": run.elapsed,
                "duration": SURVIVAL_TICKS,
                "health": run.health,
                "spawned": run.spawned,
                "dash_remaining": run.dash_remaining,
                "dash_cooldown": run.dash_cooldown,
                "dash_ready": run.dash_cooldown == 0,
                "dash_direction": {"x": run.dash_direction.0, "y": run.dash_direction.1},
                "outcome": format!("{:?}", run.outcome),
            })
        })
        .unwrap_or_default()
}
#[derive(Default)]
pub struct InteractiveInput {
    buttons: BufferedButtons<Action>,
    tracker: InputTracker<Action>,
}
impl InteractiveInput {
    pub fn clear(&mut self) {
        self.buttons.clear();
        self.tracker = InputTracker::default();
    }

    fn action(name: &str) -> Result<Action, String> {
        Ok(match name {
            "up" => Action::Up,
            "down" => Action::Down,
            "left" => Action::Left,
            "right" => Action::Right,
            "dash" => Action::Dash,
            _ => return Err(format!("unknown action: {name}")),
        })
    }

    pub fn cancel_action(&mut self, name: &str) -> Result<(), String> {
        self.buttons.cancel(&Self::action(name)?);
        Ok(())
    }

    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        let action = Self::action(name)?;
        self.buttons.set(action, pressed, action == Action::Dash);
        Ok(())
    }
    pub fn tick(&mut self, app: &mut App) {
        let mut actions = self.buttons.held().clone();
        if self.buttons.take_presses().contains(&Action::Dash) {
            // A physical release/repress may happen between fixed ticks.
            // Prime the sampler with Dash released so that edge is retained.
            self.tracker.sample(
                actions
                    .iter()
                    .copied()
                    .filter(|action| *action != Action::Dash)
                    .map(|action| (action, ActionValue::PRESSED)),
            );
            actions.insert(Action::Dash);
        }
        app.world_mut().insert_resource(
            self.tracker.sample(
                actions
                    .into_iter()
                    .map(|action| (action, ActionValue::PRESSED)),
            ),
        );
        app.advance_fixed(1);
    }
}
pub fn status(app: &App) -> String {
    let position = app
        .world()
        .iter::<Position>()
        .next()
        .map(|(_, position)| serde_json::json!({ "x": position.x, "y": position.y }));
    serde_json::json!({ "frame": app.world().resource::<FixedTime>().unwrap().tick(), "position": position, "run": run_status(app.world()) }).to_string()
}
#[cfg(not(target_arch = "wasm32"))]
fn write_ppm(path: &Path, image: &Image) -> std::io::Result<()> {
    let mut bytes = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
    let (pixels, remainder) = image.pixels().as_chunks::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        bytes.extend_from_slice(&pixel[..3]);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

pub fn image_checksum(image: &Image) -> u64 {
    image
        .pixels()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready() -> App {
        let mut a = build_game();
        a.update_schedule(Startup);
        a
    }
    fn player_position(a: &App) -> (i32, i32) {
        let entity = a.world().iter::<Player>().next().unwrap().0;
        let p = a.world().get::<Position>(entity).unwrap();
        (p.x, p.y)
    }
    #[test]
    fn enemy_inspection_metadata_remains_read_only() {
        let inspector = inspector_with_capture(
            InspectionConfig::controlled("metadata-test", "arena"),
            |_| unreachable!("capture is not used by this test"),
        );
        let components = inspector.component_field_metadata();
        let active = &components[std::any::type_name::<Enemy>()]["active"];
        assert_eq!(active.type_name, "bool");
        assert_eq!(
            active.description,
            "Whether this pooled enemy is active in the arena"
        );
        assert!(!active.writable);
        assert_eq!(
            (active.minimum, active.maximum, &active.unit),
            (None, None, &None)
        );
    }
    #[test]
    fn dash_locks_direction_and_counts_exact_cooldown() {
        let mut a = ready();
        let mut input = InteractiveInput::default();
        input.set_action("dash", true).unwrap();
        input.tick(&mut a);
        assert_eq!(player_position(&a), (84, 65));
        assert_eq!(a.world().resource::<Run>().unwrap().dash_remaining, 5);
        assert_eq!(a.world().resource::<Run>().unwrap().dash_cooldown, 120);
        input.set_action("up", true).unwrap();
        for _ in 0..5 {
            input.tick(&mut a);
        }
        assert_eq!(player_position(&a), (104, 65));
        input.set_action("up", false).unwrap();
        for _ in 0..115 {
            input.tick(&mut a);
        }
        assert_eq!(player_position(&a), (104, 65));
        assert_eq!(a.world().resource::<Run>().unwrap().dash_cooldown, 0);
        // Holding through readiness does not retrigger. Releasing then pressing
        // uses the last nonzero movement direction, even while stationary.
        input.set_action("dash", false).unwrap();
        input.tick(&mut a);
        input.set_action("dash", true).unwrap();
        input.tick(&mut a);
        assert_eq!(player_position(&a), (104, 61));
    }
    #[test]
    fn dash_rejects_early_press_and_accepts_exact_ready_tick() {
        let mut a = ready();
        let mut input = InteractiveInput::default();
        for tick in 1..=121 {
            input
                .set_action("dash", matches!(tick, 1 | 119 | 121))
                .unwrap();
            input.tick(&mut a);
            if tick == 120 {
                assert_eq!(player_position(&a), (104, 65));
                assert_eq!(a.world().resource::<Run>().unwrap().dash_cooldown, 1);
            }
        }
        assert_eq!(player_position(&a), (108, 65));
        assert_eq!(a.world().resource::<Run>().unwrap().dash_cooldown, 120);
    }
    #[test]
    fn dash_diagonal_bounds_tap_restart_and_frozen_outcome() {
        let mut a = ready();
        let mut input = InteractiveInput::default();
        input.set_action("up", true).unwrap();
        input.set_action("left", true).unwrap();
        input.set_action("dash", true).unwrap();
        input.set_action("dash", false).unwrap();
        for _ in 0..6 {
            input.tick(&mut a);
        }
        assert_eq!(player_position(&a), (56, 41), "short tap is sampled once");
        for _ in 0..115 {
            input.tick(&mut a);
        }
        input.set_action("dash", true).unwrap();
        for _ in 0..6 {
            input.tick(&mut a);
        }
        assert_eq!(player_position(&a), (0, 18));
        restart(&mut a);
        assert_eq!(a.world().resource::<Run>().unwrap().dash_cooldown, 0);
        assert_eq!(a.world().resource::<Run>().unwrap().dash_remaining, 0);
        assert_eq!(a.world().resource::<Run>().unwrap().facing, (1, 0));
        input = InteractiveInput::default();
        input.set_action("dash", true).unwrap();
        input.tick(&mut a);
        for outcome in [Outcome::Won, Outcome::Lost] {
            a.world_mut().resource_mut::<Run>().unwrap().outcome = outcome;
            let before = status(&a);
            let position = player_position(&a);
            a.advance_fixed(10);
            assert_eq!(player_position(&a), position);
            let old: serde_json::Value = serde_json::from_str(&before).unwrap();
            let new: serde_json::Value = serde_json::from_str(&status(&a)).unwrap();
            assert_eq!(old["run"], new["run"]);
        }
    }
    #[test]
    fn dash_keeps_release_repress_between_ticks_and_has_no_immunity() {
        let mut a = ready();
        let mut input = InteractiveInput::default();
        input.set_action("dash", true).unwrap();
        for _ in 0..121 {
            input.tick(&mut a);
        }
        input.set_action("dash", false).unwrap();
        input.set_action("dash", true).unwrap();
        // Contact on the first dash tick still hurts.
        let enemy = a.world().iter::<Enemy>().find(|(_, e)| e.active).unwrap().0;
        *a.world_mut().get_mut::<Position>(enemy).unwrap() = Position { x: 108, y: 65 };
        let health = a.world().resource::<Run>().unwrap().health;
        input.tick(&mut a);
        assert_eq!(player_position(&a), (108, 65));
        let run = a.world().resource::<Run>().unwrap();
        assert_eq!(run.dash_cooldown, 120);
        assert_eq!(run.health, health - 1);
    }
    #[test]
    fn resetting_interactive_input_cancels_pending_dash() {
        let mut a = ready();
        let mut input = InteractiveInput::default();
        input.set_action("dash", true).unwrap();
        input.set_action("dash", false).unwrap();
        // Focus loss and pause must discard an already released tap too.
        input.clear();
        input.tick(&mut a);
        assert_eq!(player_position(&a), (80, 65));
        assert_eq!(a.world().resource::<Run>().unwrap().dash_cooldown, 0);
    }
    #[test]
    fn idle_contact_loss_and_restart() {
        let mut a = ready();
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            0xe096abf94fd12c24
        );
        a.advance_fixed(1);
        assert_eq!(a.world().resource::<Run>().unwrap().spawned, 1);
        let e = a.world().iter::<Enemy>().find(|(_, e)| e.active).unwrap().0;
        let p = *a.world().get::<Position>(e).unwrap();
        assert_eq!((p.x, p.y), (124, 105));
        a.advance_fixed(1199);
        let r = *a.world().resource::<Run>().unwrap();
        assert_eq!((r.elapsed, r.health), (310, 0));
        assert_eq!(r.outcome, Outcome::Lost);
        let checksum = image_checksum(&render_image(a.world()).unwrap());
        a.advance_fixed(10);
        assert_eq!(image_checksum(&render_image(a.world()).unwrap()), checksum);
        let clock = a.world().resource::<FixedTime>().unwrap().tick();
        restart(&mut a);
        assert_eq!(a.world().resource::<FixedTime>().unwrap().tick(), clock);
        assert_eq!(a.world().resource::<Run>().unwrap().health, 3);
        assert_eq!(
            a.world().iter::<Enemy>().filter(|(_, e)| e.active).count(),
            0
        );
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            image_checksum(&render_image(ready().world()).unwrap())
        );
    }
    #[test]
    fn pursuit_contact_cooldown_and_arena_bounds() {
        let mut a = ready();
        a.advance_fixed(1);
        let player = a.world().iter::<Player>().next().unwrap().0;
        let enemy = a.world().iter::<Enemy>().find(|(_, e)| e.active).unwrap().0;
        *a.world_mut().get_mut::<Position>(enemy).unwrap() = Position { x: 80, y: 65 };
        a.advance_fixed(1);
        assert_eq!(a.world().resource::<Run>().unwrap().health, 2);
        a.advance_fixed(59);
        assert_eq!(a.world().resource::<Run>().unwrap().health, 2);
        a.advance_fixed(1);
        assert_eq!(a.world().resource::<Run>().unwrap().health, 1);
        restart(&mut a);
        let mut input = InteractiveInput::default();
        input.set_action("up", true).unwrap();
        input.set_action("left", true).unwrap();
        for _ in 0..100 {
            input.tick(&mut a);
        }
        let p = a.world().get::<Position>(player).unwrap();
        assert_eq!((p.x, p.y), (0, 18));
        let p = a.world().get::<Position>(enemy).unwrap();
        assert!(p.x < 124 && p.y < 105, "pursuer moves toward player");
    }
    #[test]
    fn perimeter_replay_survives_and_is_repeatable() {
        fn replay() -> App {
            let mut a = ready();
            let mut input = InteractiveInput::default();
            // Clockwise rectangle, with a deterministic opening to the upper-left lane.
            for tick in 0..SURVIVAL_TICKS {
                let action = if tick < 30 {
                    "up"
                } else if tick < 90 {
                    "right"
                } else {
                    match (tick - 90) % 360 {
                        0..=59 => "down",
                        60..=179 => "left",
                        180..=239 => "up",
                        _ => "right",
                    }
                };
                for name in ["up", "down", "left", "right"] {
                    input.set_action(name, name == action).unwrap();
                }
                input.tick(&mut a);
            }
            a
        }
        let a = replay();
        let b = replay();
        assert_eq!(
            (
                a.world().resource::<Run>().unwrap().health,
                a.world().resource::<Run>().unwrap().spawned
            ),
            (2, 5)
        );
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            0xb5cf61da6f50efd7
        );
        assert_eq!(a.world().resource::<Run>().unwrap().outcome, Outcome::Won);
        assert_eq!(status(&a), status(&b));
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            image_checksum(&render_image(b.world()).unwrap())
        );
    }
    #[test]
    fn hud_entities_follow_game_state_and_restart_uses_the_entity_button() {
        let mut app = ready();
        assert_eq!(app.world().iter::<UiNode>().count(), 3);
        let restart_button = app.world().iter::<RestartButton>().next().unwrap().0;
        assert_eq!(
            app.world().get::<UiText>(restart_button).unwrap().text,
            "R RESTART"
        );
        let mut input = InteractiveInput::default();
        input.set_action("dash", true).unwrap();
        input.tick(&mut app);
        let dash = app
            .world()
            .iter::<HudKind>()
            .find(|(_, kind)| matches!(kind, HudKind::Dash))
            .unwrap()
            .0;
        assert_eq!(app.world().get::<UiText>(dash).unwrap().text, "DASH 2.0S");
        let elapsed = app.world().resource::<Run>().unwrap().elapsed;
        let clock = app.world().resource::<FixedTime>().unwrap().tick();
        assert!(handle_ui_pointer(&mut app, Some((8, 12)), true).consumed);
        assert_eq!(
            app.world().resource::<Run>().unwrap().elapsed,
            elapsed,
            "press alone must not activate"
        );
        assert!(
            handle_ui_pointer(&mut app, Some((100, 50)), false)
                .activated
                .is_none()
        );
        assert_eq!(app.world().resource::<Run>().unwrap().elapsed, elapsed);
        handle_ui_pointer(&mut app, Some((8, 12)), true);
        cancel_ui_pointer(&mut app);
        assert!(
            handle_ui_pointer(&mut app, Some((8, 12)), false)
                .activated
                .is_none()
        );
        assert_eq!(app.world().resource::<Run>().unwrap().elapsed, elapsed);
        handle_ui_pointer(&mut app, Some((8, 12)), true);
        assert!(
            update_ui_pointer(&mut app, Some((8, 12)), false, true)
                .activated
                .is_none(),
            "local press cannot pair with controlled release"
        );
        cancel_ui_pointer(&mut app);
        update_ui_pointer(&mut app, Some((8, 12)), true, true);
        assert!(
            handle_ui_pointer(&mut app, Some((8, 12)), false)
                .activated
                .is_none(),
            "controlled press cannot pair with local release"
        );
        cancel_ui_pointer(&mut app);
        assert_eq!(app.world().resource::<Run>().unwrap().elapsed, elapsed);
        app.world_mut()
            .get_mut::<UiButton>(restart_button)
            .unwrap()
            .enabled = false;
        assert!(handle_ui_pointer(&mut app, Some((8, 12)), true).consumed);
        assert!(
            handle_ui_pointer(&mut app, Some((8, 12)), false)
                .activated
                .is_none()
        );
        app.world_mut()
            .get_mut::<UiButton>(restart_button)
            .unwrap()
            .enabled = true;
        handle_ui_pointer(&mut app, Some((8, 12)), true);
        assert_eq!(
            handle_ui_pointer(&mut app, Some((8, 12)), false).activated,
            Some(restart_button)
        );
        assert_eq!(app.world().resource::<Run>().unwrap().elapsed, 0);
        assert_eq!(app.world().resource::<FixedTime>().unwrap().tick(), clock);
        assert_eq!(app.world().get::<UiText>(dash).unwrap().text, "DASH READY");
        assert_eq!(
            image_checksum(&render_image(app.world()).unwrap()),
            0xe096_abf9_4fd1_2c24
        );
    }
}
