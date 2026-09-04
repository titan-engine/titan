//! Replace this module with your game. Hosts depend only on the functions below.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::{Path, PathBuf},
};
use titan::input::{ActionValue, InputFrame, InputTracker};
use titan::inspection::{InspectionConfig, Inspector};
use titan::render::{
    Color, Image, ImageAssets, ImageId, RenderFrame, SoftwareRenderer, SpriteDraw,
};
use titan::{App, Component, FixedTime, FixedUpdate, Name, Res, ResMut, Startup, World};
use titan_protocol::{
    CaptureResult, CommandMetadata, ErrorCode, FieldMetadata, InputValue, ProtocolError,
};

pub const WIDTH: i32 = 160;
pub const HEIGHT: i32 = 112;
const DOT_SIZE: i32 = 7;
pub const SEED: u32 = 0xA2E4;
pub const SURVIVAL_TICKS: u32 = 1200;
#[derive(Component, Clone, Copy)]
struct Enemy {
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
struct Art {
    player: ImageId,
    enemy: ImageId,
    floor: ImageId,
    glyphs: BTreeMap<char, ImageId>,
}

pub fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, apply_scheduled_input);
    app.add_systems(FixedUpdate, simulate);
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
    app.refresh_extracted();
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
                            "movement actions require button values",
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
    let mut glyphs = BTreeMap::new();
    for (c, rows) in [
        ('0', [7, 5, 5, 5, 7]),
        ('1', [2, 6, 2, 2, 7]),
        ('2', [7, 1, 7, 4, 7]),
        ('3', [7, 1, 7, 1, 7]),
        ('4', [5, 5, 7, 1, 1]),
        ('5', [7, 4, 7, 1, 7]),
        ('6', [7, 4, 7, 5, 7]),
        ('7', [7, 1, 1, 1, 1]),
        ('8', [7, 5, 7, 5, 7]),
        ('9', [7, 5, 7, 1, 7]),
        ('H', [5, 5, 7, 5, 5]),
        ('P', [6, 5, 6, 4, 4]),
        ('T', [7, 2, 2, 2, 2]),
        ('I', [7, 2, 2, 2, 7]),
        ('M', [5, 7, 7, 5, 5]),
        ('E', [7, 4, 6, 4, 7]),
        ('W', [5, 5, 7, 7, 5]),
        ('O', [7, 5, 5, 5, 7]),
        ('N', [5, 7, 7, 7, 5]),
        ('L', [4, 4, 4, 4, 7]),
        ('S', [7, 4, 7, 1, 7]),
        ('R', [6, 5, 6, 5, 5]),
        ('A', [2, 5, 7, 5, 5]),
        ('/', [1, 1, 2, 4, 4]),
    ] {
        glyphs.insert(
            c,
            assets.insert(
                Image::from_fn(3, 5, |x, y| {
                    if rows[y as usize] & (1 << (2 - x)) != 0 {
                        Color::rgb(225, 239, 249)
                    } else {
                        Color::rgba(0, 0, 0, 0)
                    }
                })
                .unwrap(),
            ),
        );
    }
    world.insert_resource(assets);
    world.insert_resource(Art {
        player,
        enemy,
        floor,
        glyphs,
    });
    world.insert_resource(Run::default());
    world.spawn_with((initial_position(), Player, Name::new("player")));
    for index in 0..14 {
        world.spawn_with((
            Position { x: 0, y: 18 },
            Enemy { active: false },
            Name::new(format!("enemy-{index}")),
        ));
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
        let run = world.resource::<Run>().unwrap();
        let hud = format!("HP {}   TIME {:02}/20", run.health, run.elapsed / 60);
        let outcome = match run.outcome {
            Outcome::Running => "R RESTART",
            Outcome::Won => "WON R RESTART",
            Outcome::Lost => "LOST R RESTART",
        };
        for (line, y) in [(hud.as_str(), 3), (outcome, 10)] {
            for (i, c) in line.chars().enumerate() {
                if let Some(id) = art.glyphs.get(&c) {
                    frame.push(SpriteDraw::new(*id, 4 + i as i32 * 4, y));
                }
            }
        }
    }
    frame
}
fn run_status(world: &World) -> serde_json::Value {
    world.resource::<Run>().map(|run| serde_json::json!({"seed":SEED,"elapsed":run.elapsed,"duration":SURVIVAL_TICKS,"health":run.health,"spawned":run.spawned,"outcome":format!("{:?}",run.outcome)})).unwrap_or_default()
}
#[derive(Default)]
pub struct InteractiveInput {
    held: BTreeSet<Action>,
    tracker: InputTracker<Action>,
}
impl InteractiveInput {
    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        let action = match name {
            "up" => Action::Up,
            "down" => Action::Down,
            "left" => Action::Left,
            "right" => Action::Right,
            _ => return Err(format!("unknown action: {name}")),
        };
        if pressed {
            self.held.insert(action);
        } else {
            self.held.remove(&action);
        }
        Ok(())
    }
    pub fn tick(&mut self, app: &mut App) {
        app.world_mut().insert_resource(
            self.tracker.sample(
                self.held
                    .iter()
                    .copied()
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
    #[test]
    fn idle_contact_loss_and_restart() {
        let mut a = ready();
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            0x1e5d_05f5_47d5_3435
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
            0xbe61_b1c7_10b1_01b6
        );
        assert_eq!(a.world().resource::<Run>().unwrap().outcome, Outcome::Won);
        assert_eq!(status(&a), status(&b));
        assert_eq!(
            image_checksum(&render_image(a.world()).unwrap()),
            image_checksum(&render_image(b.world()).unwrap())
        );
    }
}
