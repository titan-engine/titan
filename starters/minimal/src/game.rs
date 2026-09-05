//! Replace this module with your game. Hosts depend only on the functions below.
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
use titan::{App, Component, FixedTime, FixedUpdate, Name, Query, Res, ResMut, Startup, World};
use titan_protocol::{
    CaptureResult, CommandMetadata, ErrorCode, FieldMetadata, InputValue, ProtocolError,
};

pub const WIDTH: i32 = 160;
pub const HEIGHT: i32 = 112;
const DOT_SIZE: i32 = 5;
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
struct Art(ImageId);

pub fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, apply_scheduled_input);
    app.add_systems(FixedUpdate, move_player);
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
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    app.refresh_extracted();
}
fn initial_position() -> Position {
    Position {
        x: WIDTH / 2,
        y: HEIGHT / 2,
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
                    description: "Dot pixel coordinate".into(),
                    writable: true,
                    minimum: Some(0.0),
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
                })
            })
        })
        .take(1000)
        .collect();
    serde_json::json!(positions)
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
    let dot = assets.insert(
        Image::from_fn(DOT_SIZE as u32, DOT_SIZE as u32, |_, _| {
            Color::rgb(90, 220, 230)
        })
        .unwrap(),
    );
    world.insert_resource(assets);
    world.insert_resource(Art(dot));
    world.spawn_with((initial_position(), Player, Name::new("player")));
}
fn move_player(mut players: Query<(&mut Position, &Player)>, input: Res<InputFrame<Action>>) {
    let x = i32::from(input.is_active(&Action::Right)) - i32::from(input.is_active(&Action::Left));
    let y = i32::from(input.is_active(&Action::Down)) - i32::from(input.is_active(&Action::Up));
    players.for_each(|_, (position, _)| {
        position.x = (position.x + x).clamp(0, WIDTH - DOT_SIZE);
        position.y = (position.y + y).clamp(0, HEIGHT - DOT_SIZE);
    });
}
fn render_frame(world: &World) -> RenderFrame {
    let mut frame = RenderFrame::new(WIDTH as u32, HEIGHT as u32, Color::rgb(24, 30, 44));
    if let Some(art) = world.resource::<Art>() {
        for (_, position) in world.iter::<Position>() {
            frame.push(SpriteDraw::new(art.0, position.x, position.y));
        }
    }
    frame
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
            _ => return Err(format!("unknown action: {name}")),
        })
    }

    pub fn cancel_action(&mut self, name: &str) -> Result<(), String> {
        self.buttons.cancel(&Self::action(name)?);
        Ok(())
    }

    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        let action = Self::action(name)?;
        self.buttons.set(action, pressed, false);
        Ok(())
    }
    pub fn tick(&mut self, app: &mut App) {
        app.world_mut().insert_resource(
            self.tracker.sample(
                self.buttons
                    .held()
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
    serde_json::json!({ "frame": app.world().resource::<FixedTime>().unwrap().tick(), "position": position }).to_string()
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
