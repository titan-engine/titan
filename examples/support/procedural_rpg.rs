use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;

use serde::Deserialize;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use titan::inspection::{InspectionConfig, Inspector};
use titan_protocol::{
    CaptureResult, CommandMetadata, ErrorCode, FieldMetadata, InputValue, ProtocolError,
};

use titan::input::{ActionValue, InputFrame, InputTracker};
#[cfg(not(target_arch = "wasm32"))]
use titan::input::{InputRecording, RecordingHeader};
use titan::render::{
    Color, Image, ImageAssets, ImageId, RenderFrame, SoftwareRenderer, SpriteDraw,
};
use titan::{App, Component, FixedTime, FixedUpdate, Name, Startup, World};

const TILE_SIZE: i32 = 8;
const MAP_WIDTH: i32 = 20;
const MAP_HEIGHT: i32 = 14;

#[derive(Component, Clone, Copy)]
struct Position {
    x: i32,
    y: i32,
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Shard;

#[derive(Component)]
struct Shrine;

#[derive(Component)]
struct ActiveShrine;

#[derive(Default)]
struct ScheduledInput {
    frames: BTreeMap<u64, Vec<(Action, ActionValue)>>,
    tracker: InputTracker<Action>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnShardArgs {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Default)]
pub struct QuestState {
    pub collected_shards: usize,
    pub shrine_active: bool,
}

#[derive(Clone, Copy)]
struct Art {
    grass: [ImageId; 2],
    player: ImageId,
    shard: ImageId,
    shrine_inactive: ImageId,
    shrine_active: ImageId,
}

#[allow(dead_code)]
struct ExtractedFrame(RenderFrame);

pub fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(QuestState::default());
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, apply_scheduled_input);
    app.add_systems(FixedUpdate, move_player);
    app.add_systems(FixedUpdate, collect_shards);
    app.add_systems(FixedUpdate, extract_frame);
    app
}

// Registration is independent of transport: callers execute requests between ticks.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_inspector(output_path: PathBuf) -> Inspector {
    configured_inspector(
        output_path,
        InspectionConfig::controlled("procedural-rpg", "procedural-rpg"),
    )
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
    inspector
        .register_command(
            CommandMetadata {
                name: "spawn_shard".into(),
                description: "Spawn a collectible shard at a map tile.".into(),
                arguments: [("x", MAP_WIDTH), ("y", MAP_HEIGHT)]
                    .into_iter()
                    .map(|(name, limit)| {
                        (
                            name.into(),
                            FieldMetadata {
                                type_name: "i32".into(),
                                description: "Map tile coordinate".into(),
                                writable: true,
                                minimum: Some(0.0),
                                maximum: Some(f64::from(limit - 1)),
                                unit: Some("tile".into()),
                            },
                        )
                    })
                    .collect(),
            },
            |app, args: SpawnShardArgs| {
                if !(0..MAP_WIDTH).contains(&args.x) || !(0..MAP_HEIGHT).contains(&args.y) {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidValue,
                        "shard coordinates are outside the map",
                    ));
                }
                app.update_schedule(Startup);
                spawn_at(
                    app.world_mut(),
                    Position {
                        x: args.x,
                        y: args.y,
                    },
                    Shard,
                    "spawned-shard",
                );
                Ok(())
            },
        )
        .expect("unique command name");
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
        if app.world().resource::<ScheduledInput>().is_none() {
            app.world_mut().insert_resource(ScheduledInput::default());
        }
        app.world_mut()
            .resource_mut::<ScheduledInput>()
            .unwrap()
            .frames
            .insert(frame, values);
        Ok(())
    });
    inspector.register_capture_handler(capture);
    inspector
}

pub fn render_image(world: &World) -> Result<Image, ProtocolError> {
    let assets = world.resource::<ImageAssets>().ok_or_else(|| {
        ProtocolError::new(ErrorCode::Busy, "run startup with step before capturing")
    })?;
    SoftwareRenderer::render(&render_frame(world), assets).map_err(|error| {
        ProtocolError::new(ErrorCode::Internal, format!("render failed: {error:?}"))
    })
}

fn apply_scheduled_input(world: &mut World) {
    let frame = world.resource::<FixedTime>().unwrap().tick() + 1;
    if let Some(scheduled) = world.resource_mut::<ScheduledInput>() {
        // Each submitted map is a complete snapshot for one completed frame.
        let values = scheduled.frames.remove(&frame).unwrap_or_default();
        let input = scheduled.tracker.sample(values);
        world.insert_resource(input);
    }
}

fn setup(world: &mut World) {
    let mut assets = ImageAssets::new();
    let art = generate_art(&mut assets);
    world.insert_resource(assets);
    world.insert_resource(art);

    spawn_at(world, Position { x: 2, y: 2 }, Player, "player");
    spawn_at(world, Position { x: 4, y: 2 }, Shard, "shard-1");
    spawn_at(world, Position { x: 4, y: 5 }, Shard, "shard-2");
    spawn_at(world, Position { x: 8, y: 5 }, Shard, "shard-3");
    spawn_at(world, Position { x: 10, y: 5 }, Shrine, "shrine");
}

fn spawn_at<T: Component>(world: &mut World, position: Position, marker: T, name: &str) {
    let entity = world.spawn();
    world.insert(entity, position).unwrap();
    world.insert(entity, marker).unwrap();
    world.insert(entity, Name::new(name)).unwrap();
}

fn move_player(world: &mut World) {
    let input = world.resource::<InputFrame<Action>>().unwrap();
    let x = i32::from(input.is_active(&Action::Right)) - i32::from(input.is_active(&Action::Left));
    let y = i32::from(input.is_active(&Action::Down)) - i32::from(input.is_active(&Action::Up));
    let player = world
        .iter::<Player>()
        .next()
        .map(|(entity, _)| entity)
        .unwrap();
    let position = world.get_mut::<Position>(player).unwrap();
    position.x = (position.x + x).clamp(0, MAP_WIDTH - 1);
    position.y = (position.y + y).clamp(0, MAP_HEIGHT - 1);
}

fn collect_shards(world: &mut World) {
    let player_position = world
        .iter::<Player>()
        .next()
        .and_then(|(entity, _)| world.get::<Position>(entity))
        .copied()
        .unwrap();
    let collected: Vec<_> = world
        .iter::<Shard>()
        .filter_map(|(entity, _)| {
            let position = world.get::<Position>(entity)?;
            (position.x == player_position.x && position.y == player_position.y).then_some(entity)
        })
        .collect();

    if collected.is_empty() {
        return;
    }

    let state = world.resource_mut::<QuestState>().unwrap();
    state.collected_shards += collected.len();
    state.shrine_active = state.collected_shards >= 3;
    if state.shrine_active {
        let shrine = world.iter::<Shrine>().next().unwrap().0;
        world.insert(shrine, ActiveShrine).unwrap();
    }
    let mut commands = world.commands();
    for shard in collected {
        commands.despawn(shard);
    }
}

fn extract_frame(world: &mut World) {
    world.insert_resource(ExtractedFrame(render_frame(world)));
}

fn render_frame(world: &World) -> RenderFrame {
    let art = *world.resource::<Art>().unwrap();
    let shrine_active = world.resource::<QuestState>().unwrap().shrine_active;
    let mut frame = RenderFrame::new(
        (MAP_WIDTH * TILE_SIZE) as u32,
        (MAP_HEIGHT * TILE_SIZE) as u32,
        Color::rgb(12, 20, 18),
    );

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let variation = terrain_variation(0x0054_4954_414e, x, y) as usize;
            frame.push(SpriteDraw::new(
                art.grass[variation],
                x * TILE_SIZE,
                y * TILE_SIZE,
            ));
        }
    }

    for (entity, position) in world.iter::<Position>() {
        let image = if world.get::<Player>(entity).is_some() {
            art.player
        } else if world.get::<Shard>(entity).is_some() {
            art.shard
        } else if world.get::<Shrine>(entity).is_some() {
            if shrine_active {
                art.shrine_active
            } else {
                art.shrine_inactive
            }
        } else {
            continue;
        };
        frame.push(
            SpriteDraw::new(image, position.x * TILE_SIZE, position.y * TILE_SIZE).with_layer(1),
        );
    }

    frame
}

fn generate_art(assets: &mut ImageAssets) -> Art {
    let grass = [
        assets.insert(
            Image::from_fn(8, 8, |x, y| {
                if terrain_variation(11, x as i32, y as i32) == 0 {
                    Color::rgb(49, 111, 62)
                } else {
                    Color::rgb(43, 98, 55)
                }
            })
            .unwrap(),
        ),
        assets.insert(
            Image::from_fn(8, 8, |x, y| {
                if (x + y * 3) % 7 == 0 {
                    Color::rgb(62, 127, 68)
                } else {
                    Color::rgb(47, 105, 58)
                }
            })
            .unwrap(),
        ),
    ];
    let player = assets.insert(
        Image::from_fn(8, 8, |x, y| match (x, y) {
            (2..=5, 1) => Color::rgb(63, 38, 52),
            (2 | 5, 3) => Color::rgb(238, 208, 159),
            (1..=6, 2..=3) => Color::rgb(91, 51, 63),
            (2..=5, 4..=5) => Color::rgb(53, 91, 154),
            (2 | 5, 6..=7) => Color::rgb(31, 42, 58),
            _ => Color::TRANSPARENT,
        })
        .unwrap(),
    );
    let shard = assets.insert(
        Image::from_fn(8, 8, |x, y| {
            let distance = x.abs_diff(3) + y.abs_diff(3);
            match distance {
                0..=1 => Color::rgb(238, 255, 255),
                2..=3 => Color::rgb(91, 221, 226),
                4 if x > 1 && y > 1 => Color::rgb(35, 130, 164),
                _ => Color::TRANSPARENT,
            }
        })
        .unwrap(),
    );
    let shrine = |active| {
        Image::from_fn(8, 8, |x, y| match (x, y) {
            (2..=5, 0..=1) if active => Color::rgb(255, 224, 100),
            (1..=6, 2) => Color::rgb(94, 76, 89),
            (2..=5, 3..=6) if active => Color::rgb(103, 224, 190),
            (2..=5, 3..=6) => Color::rgb(48, 62, 70),
            (1..=6, 7) => Color::rgb(67, 54, 63),
            _ => Color::TRANSPARENT,
        })
        .unwrap()
    };
    Art {
        grass,
        player,
        shard,
        shrine_inactive: assets.insert(shrine(false)),
        shrine_active: assets.insert(shrine(true)),
    }
}

fn terrain_variation(seed: u64, x: i32, y: i32) -> u32 {
    let mut value = seed ^ (x as u64).wrapping_mul(0x9e37_79b9) ^ (y as u64).rotate_left(17);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    (value ^ (value >> 31)) as u32 & 1
}

#[cfg(not(target_arch = "wasm32"))]
pub fn recorded_walk() -> InputRecording<Action> {
    let mut tracker = InputTracker::new();
    let mut recording = InputRecording::new(RecordingHeader::new(
        16_666_667,
        0x0054_4954_414e,
        0x5250_475f_5631,
    ));
    for (action, ticks) in [(Action::Right, 2), (Action::Down, 3), (Action::Right, 6)] {
        for _ in 0..ticks {
            recording.push(tracker.sample([(action, ActionValue::PRESSED)]));
        }
    }
    recording
}

#[cfg(not(target_arch = "wasm32"))]
pub fn replay(app: &mut App, recording: &InputRecording<Action>) {
    for frame in recording.frames() {
        *app.world_mut()
            .resource_mut::<InputFrame<Action>>()
            .unwrap() = frame.clone();
        app.advance_fixed(1);
    }
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{QuestState, build_game, image_checksum, recorded_walk, replay};
    use titan::render::{ImageAssets, SoftwareRenderer};

    #[test]
    fn recorded_walk_collects_every_shard_and_activates_the_shrine() {
        let mut app = build_game();
        let recording = recorded_walk();
        replay(&mut app, &recording);

        let quest = app.world().resource::<QuestState>().unwrap();
        assert_eq!(quest.collected_shards, 3);
        assert!(quest.shrine_active);
        assert_eq!(recording.len(), 11);

        let frame = &app.world().resource::<super::ExtractedFrame>().unwrap().0;
        let image = SoftwareRenderer::render(frame, app.world().resource::<ImageAssets>().unwrap())
            .unwrap();
        assert_eq!(image_checksum(&image), 0x9861_8cd7_21c5_b52d);
    }

    fn request(
        app: &mut titan::App,
        inspector: &mut titan::inspection::Inspector,
        request: titan_protocol::Request,
    ) -> titan_protocol::ResponseEnvelope {
        inspector.handle(
            app,
            &titan_protocol::RequestEnvelope::new("acceptance", request),
        )
    }

    fn success(response: titan_protocol::ResponseEnvelope) -> titan_protocol::Response {
        match response.outcome {
            titan_protocol::ResponseOutcome::Success { response } => response,
            other => panic!("expected success: {other:?}"),
        }
    }

    #[test]
    fn protocol_drives_inspection_replay_command_and_reference_capture() {
        use titan_protocol::{EntityQuery, InputValue, PageRequest, Request, Response};
        let path =
            std::env::temp_dir().join(format!("titan-rpg-{}-reference.ppm", std::process::id()));
        let mut app = build_game();
        let mut inspector = super::build_inspector(path.clone());
        let Response::Capabilities(capabilities) =
            success(request(&mut app, &mut inspector, Request::Capabilities))
        else {
            panic!("expected capabilities");
        };
        for operation in [
            titan_protocol::Operation::Invoke,
            titan_protocol::Operation::InjectInput,
            titan_protocol::Operation::Capture,
        ] {
            assert!(capabilities.operations.contains(&operation));
        }

        success(request(
            &mut app,
            &mut inspector,
            Request::Step { frames: 0 },
        ));
        let Response::Entities(page) = success(request(
            &mut app,
            &mut inspector,
            Request::Entities {
                query: EntityQuery::default(),
                page: PageRequest::default(),
            },
        )) else {
            panic!("expected entities")
        };
        assert_eq!(page.entities.len(), 5);
        assert!(
            page.entities
                .iter()
                .any(|entity| entity.name.as_deref() == Some("player"))
        );
        let shrine = page
            .entities
            .iter()
            .find(|entity| entity.name.as_deref() == Some("shrine"))
            .unwrap()
            .id;
        let mut frame = 0;
        for (action, ticks) in [("right", 2), ("down", 3), ("right", 6)] {
            for _ in 0..ticks {
                frame += 1;
                let response = request(
                    &mut app,
                    &mut inspector,
                    Request::InjectInput {
                        frame,
                        actions: [(action.into(), InputValue::Button(true))].into(),
                    },
                );
                assert_eq!(response.observed_frame, 0);
                assert_eq!(
                    success(response),
                    Response::Applied {
                        applied_frame: frame
                    }
                );
            }
        }
        let response = request(&mut app, &mut inspector, Request::Step { frames: 11 });
        assert_eq!(response.observed_frame, 11);
        success(response);
        let Response::Entity(details) = success(request(
            &mut app,
            &mut inspector,
            Request::Entity { entity: shrine },
        )) else {
            panic!("expected shrine")
        };
        assert!(
            details
                .components
                .keys()
                .any(|name| name.ends_with("::ActiveShrine"))
        );
        let Response::Entities(page) = success(request(
            &mut app,
            &mut inspector,
            Request::Entities {
                query: EntityQuery::default(),
                page: PageRequest::default(),
            },
        )) else {
            panic!("expected entities")
        };
        assert_eq!(page.entities.len(), 2);
        let Response::Capture(capture) =
            success(request(&mut app, &mut inspector, Request::Capture))
        else {
            panic!("expected capture")
        };
        assert_eq!((capture.width, capture.height), (160, 112));
        assert_eq!(capture.checksum, "98618cd721c5b52d");
        assert_eq!(capture.artifact, path.to_string_lossy());
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"P6\n160 112\n255\n"));
        assert_eq!(bytes.len(), b"P6\n160 112\n255\n".len() + 160 * 112 * 3);
        let Response::Commands { commands } =
            success(request(&mut app, &mut inspector, Request::Commands))
        else {
            panic!("expected commands")
        };
        assert_eq!(commands[0].name, "spawn_shard");
        success(request(
            &mut app,
            &mut inspector,
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: [("x".into(), 0.into()), ("y".into(), 0.into())].into(),
            },
        ));
        let Response::Capture(changed) =
            success(request(&mut app, &mut inspector, Request::Capture))
        else {
            panic!("expected capture")
        };
        assert_ne!(
            changed.checksum, capture.checksum,
            "capture must reflect commands without an extra tick"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejected_requests_leave_state_unchanged_and_capture_io_is_structured() {
        use titan_protocol::{ErrorCode, InputValue, Request, ResponseOutcome};
        let path =
            std::env::temp_dir().join(format!("titan-rpg-{}-capture-blocker", std::process::id()));
        std::fs::write(&path, b"file blocks capture directory").unwrap();
        let mut app = build_game();
        let mut inspector = super::build_inspector(path.join("capture.ppm"));
        success(request(
            &mut app,
            &mut inspector,
            Request::Step { frames: 0 },
        ));
        for invalid in [
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: [("x".into(), (-1).into()), ("y".into(), 0.into())].into(),
            },
            Request::Invoke {
                name: "spawn_shard".into(),
                arguments: [("x".into(), "bad".into()), ("y".into(), 0.into())].into(),
            },
            Request::InjectInput {
                frame: 0,
                actions: Default::default(),
            },
            Request::InjectInput {
                frame: 1,
                actions: [("fly".into(), InputValue::Button(true))].into(),
            },
            Request::InjectInput {
                frame: 1,
                actions: [("right".into(), InputValue::Axis(10))].into(),
            },
        ] {
            let response = request(&mut app, &mut inspector, invalid);
            assert_eq!(response.state_revision, 1);
            assert!(
                matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
            );
        }
        assert_eq!(app.world().entities().count(), 5);
        assert!(app.world().resource::<super::ScheduledInput>().is_none());
        let response = request(&mut app, &mut inspector, Request::Capture);
        assert_eq!(response.state_revision, 1);
        assert!(
            matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::Internal)
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn scheduled_input_applies_once_at_its_requested_frame() {
        use titan_protocol::{InputValue, Request};
        let mut app = build_game();
        let mut inspector = super::build_inspector("unused.ppm".into());
        success(request(
            &mut app,
            &mut inspector,
            Request::InjectInput {
                frame: 2,
                actions: [("right".into(), InputValue::Button(true))].into(),
            },
        ));
        success(request(
            &mut app,
            &mut inspector,
            Request::Step { frames: 1 },
        ));
        let player = app.world().iter::<super::Player>().next().unwrap().0;
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 2);
        success(request(
            &mut app,
            &mut inspector,
            Request::Step { frames: 2 },
        ));
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 3);
        let input = app
            .world()
            .resource::<titan::input::InputFrame<super::Action>>()
            .unwrap();
        assert!(!input.is_active(&super::Action::Right));
        assert!(input.just_released(&super::Action::Right));
    }
}
