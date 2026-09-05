#[path = "rpg_assets.rs"]
pub mod assets;
#[path = "rpg_journal.rs"]
pub mod journal;
#[path = "rpg_live.rs"]
pub mod live;
#[path = "rpg_snapshot.rs"]
pub mod snapshot;

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

use titan::input::{ActionValue, BufferedButtons, InputFrame, InputTracker};
use titan::input::{InputRecording, RecordingHeader};
use titan::render::{
    Color, Image, ImageAssets, ImageId, RenderFrame, SoftwareRenderer, SpriteDraw,
};
use titan::ui::{BitmapFont, UiNode, UiText, append_ui_filtered, register_ui_inspection};
use titan::{
    App, Commands, Component, FixedTime, FixedUpdate, Name, Query, Res, ResMut, Startup, World,
};

const TILE_SIZE: i32 = 8;
const MAP_WIDTH: i32 = 20;
const MAP_HEIGHT: i32 = 14;

#[derive(Component, titan::Inspect, Clone, Copy)]
struct Position {
    /// Map tile coordinate
    #[inspect(writable, minimum = 0, maximum = MAP_WIDTH - 1, unit = "tile")]
    x: i32,
    /// Map tile coordinate
    #[inspect(writable, minimum = 0, maximum = MAP_HEIGHT - 1, unit = "tile")]
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

#[derive(Component)]
struct QuestHud;

#[derive(Default)]
struct ScheduledInput {
    enabled: bool,
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
    meadow: ImageId,
    tree: ImageId,
    flowers: ImageId,
    rock: ImageId,
    player: ImageId,
    shard: ImageId,
    shrine_inactive: ImageId,
    shrine_active: ImageId,
}

struct PlayerImage(Image);

pub fn player_image(world: &World) -> &Image {
    &world.resource::<PlayerImage>().expect("RPG player image").0
}

pub fn build_game() -> App {
    build_game_with_player(generated_player())
}

pub fn build_game_with_player(image: Image) -> App {
    let mut app = App::new();
    app.world_mut().insert_resource(PlayerImage(image));
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(QuestState::default());
    app.world_mut().insert_resource(ScheduledInput::default());
    app.add_systems(Startup, setup);
    app.add_systems(Startup, live::begin_recording);
    app.add_systems(FixedUpdate, apply_scheduled_input);
    app.add_systems(FixedUpdate, live::record_consumed);
    app.add_systems(FixedUpdate, move_player);
    app.add_systems(FixedUpdate, collect_shards);
    app.add_systems(FixedUpdate, activate_shrine);
    app.add_systems(FixedUpdate, sync_quest_ui);
    app.add_systems(FixedUpdate, journal::sync_labels);
    app.add_systems(FixedUpdate, titan::ApplyDeferred);
    app.add_systems(FixedUpdate, live::finish_tick);
    app.add_extractor(render_frame);
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
    register_ui_inspection(&mut inspector).expect("unique UI fields");
    inspector
        .register_inspectable::<Position>()
        .expect("unique position fields");

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
        let scheduled = app.world_mut().resource_mut::<ScheduledInput>().unwrap();
        scheduled.enabled = true;
        scheduled.frames.insert(frame, values);
        Ok(())
    });
    inspector.register_capture_handler(capture);
    live::register(&mut inspector);
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

pub fn render_replay_image(world: &World) -> Result<Image, ProtocolError> {
    let assets = world
        .resource::<ImageAssets>()
        .ok_or_else(|| ProtocolError::new(ErrorCode::Busy, "run startup before capturing"))?;
    SoftwareRenderer::render(&render_frame_view(world, false), assets).map_err(|error| {
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
    let art = generate_art(&mut assets, player_image(world));
    let font = BitmapFont::tiny(&mut assets);
    world.insert_resource(assets);
    world.insert_resource(font);
    world.insert_resource(art);

    spawn_at(world, Position { x: 2, y: 2 }, Player, "player");
    spawn_at(world, Position { x: 4, y: 2 }, Shard, "shard-1");
    spawn_at(world, Position { x: 4, y: 5 }, Shard, "shard-2");
    spawn_at(world, Position { x: 8, y: 5 }, Shard, "shard-3");
    spawn_at(world, Position { x: 10, y: 5 }, Shrine, "shrine");
    world.spawn_with((
        Name::new("ui/quest"),
        QuestHud,
        UiNode::new(4, 4, 152, 5),
        UiText::new("SHARDS 0/3").with_color(Color::rgb(255, 248, 217)),
    ));
    journal::setup(world);
}

fn sync_quest_ui(state: Res<QuestState>, mut labels: Query<(&mut UiText, &QuestHud)>) {
    let text = format!(
        "SHARDS {}/3{}",
        state.collected_shards,
        if state.shrine_active {
            "  SHRINE ACTIVE"
        } else {
            ""
        },
    );
    labels.for_each(|_, (label, _)| label.text.clone_from(&text));
}

fn spawn_at<T: Component>(world: &mut World, position: Position, marker: T, name: &str) {
    world.spawn_with((position, marker, Name::new(name)));
}

fn move_player(mut players: Query<(&mut Position, &Player)>, input: Res<InputFrame<Action>>) {
    let x = i32::from(input.is_active(&Action::Right)) - i32::from(input.is_active(&Action::Left));
    let y = i32::from(input.is_active(&Action::Down)) - i32::from(input.is_active(&Action::Up));
    players.for_each(|_, (position, _)| {
        position.x = (position.x + x).clamp(0, MAP_WIDTH - 1);
        position.y = (position.y + y).clamp(0, MAP_HEIGHT - 1);
    });
}

fn collect_shards(
    mut players: Query<(&Position, &Player)>,
    mut shards: Query<(&Position, &Shard)>,
    mut state: ResMut<QuestState>,
    mut commands: Commands,
) {
    let mut player_position = None;
    players.for_each_sorted(|_, (position, _)| {
        player_position.get_or_insert(*position);
    });
    let Some(player_position) = player_position else {
        return;
    };
    shards.for_each_sorted(|entity, (position, _)| {
        if position.x == player_position.x && position.y == player_position.y {
            state.collected_shards += 1;
            commands.despawn(entity);
        }
    });
    state.shrine_active = state.collected_shards >= 3;
}

fn activate_shrine(mut shrines: Query<&Shrine>, state: Res<QuestState>, mut commands: Commands) {
    if state.shrine_active {
        shrines.for_each_sorted(|entity, _| {
            commands.insert(entity, ActiveShrine);
        });
    }
}

fn render_frame(world: &World) -> RenderFrame {
    render_frame_view(world, true)
}

fn render_frame_view(world: &World, show_journal: bool) -> RenderFrame {
    let art = *world.resource::<Art>().unwrap();
    let shrine_active = world.resource::<QuestState>().unwrap().shrine_active;
    let mut frame = RenderFrame::new(
        (MAP_WIDTH * TILE_SIZE) as u32,
        (MAP_HEIGHT * TILE_SIZE) as u32,
        Color::rgb(103, 150, 87),
    );
    frame.push(SpriteDraw::new(art.meadow, 0, 0));

    // Scenery is presentation only. Large silhouettes frame the border; the
    // reference walk stays open, and all gameplay sprites draw above scenery.
    for (x, y) in [
        (-6, -3),
        (16, -5),
        (39, -5),
        (64, -4),
        (92, -6),
        (118, -5),
        (143, -2),
        (-7, 30),
        (150, 31),
        (-7, 73),
        (150, 70),
        (9, 93),
        (38, 94),
        (72, 93),
        (107, 92),
        (139, 92),
    ] {
        frame.push(SpriteDraw::new(art.tree, x, y));
    }
    for (x, y) in [
        (15, 44),
        (52, 22),
        (102, 26),
        (124, 50),
        (42, 70),
        (93, 83),
        (136, 91),
    ] {
        frame.push(SpriteDraw::new(art.flowers, x, y));
    }
    for (x, y) in [(21, 77), (61, 63), (112, 70), (133, 23)] {
        frame.push(SpriteDraw::new(art.rock, x, y));
    }

    for (entity, position) in world.iter::<Position>() {
        let (image, offset_x, offset_y, layer) = if world.get::<Player>(entity).is_some() {
            (art.player, 0, -3, 3)
        } else if world.get::<Shard>(entity).is_some() {
            (art.shard, 0, -2, 2)
        } else if world.get::<Shrine>(entity).is_some() {
            // The monument rises behind its interaction tile, keeping both the
            // lit rune and the player visible when the quest completes.
            (
                if shrine_active {
                    art.shrine_active
                } else {
                    art.shrine_inactive
                },
                -3,
                -12,
                1,
            )
        } else {
            continue;
        };
        frame.push(
            SpriteDraw::new(
                image,
                position.x * TILE_SIZE + offset_x,
                position.y * TILE_SIZE + offset_y,
            )
            .with_layer(layer),
        );
    }
    if show_journal {
        journal::append_background(world, &mut frame);
    }
    append_ui_filtered(world, &mut frame, |entity| {
        show_journal || world.get::<journal::JournalNode>(entity).is_none()
    });
    frame
}

fn generate_art(assets: &mut ImageAssets, player_image: &Image) -> Art {
    let meadow = assets.insert(
        Image::from_fn(
            (MAP_WIDTH * TILE_SIZE) as u32,
            (MAP_HEIGHT * TILE_SIZE) as u32,
            |x, y| {
                let (x, y) = (x as i32, y as i32);
                let path = meadow_path(x, y);
                // Match the reference recording's seed; no runtime randomness.
                let mut noise = 0x0054_4954_414e_u64
                    ^ (x as u64).wrapping_mul(0x9e37_79b9)
                    ^ (y as u64).rotate_left(17);
                noise ^= noise >> 30;
                noise = noise.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                let noise = (noise ^ (noise >> 27)) % 101;
                if path {
                    if !meadow_path(x - 1, y) || !meadow_path(x, y - 1) {
                        Color::rgb(151, 151, 91)
                    } else if !meadow_path(x + 1, y) || !meadow_path(x, y + 1) {
                        Color::rgb(178, 161, 108)
                    } else if noise < 3 {
                        Color::rgb(215, 197, 143)
                    } else {
                        Color::rgb(194, 175, 120)
                    }
                } else if noise < 7 && x % 2 == 0 {
                    Color::rgb(117, 167, 93)
                } else if noise > 97 && y % 3 == 0 {
                    Color::rgb(88, 134, 76)
                } else {
                    Color::rgb(103, 150, 87)
                }
            },
        )
        .unwrap(),
    );
    let tree = pixel_art(
        assets,
        &[
            "......dddddd......",
            "....ddlllllldd....",
            "...dllhhhhhllld...",
            "..dllhhhllllllld..",
            ".dllhhhlllllllldd.",
            ".dllhhlllllhhllld.",
            "dlllhlllllhhhlllld",
            "dlllhllllllhllllld",
            "dlllllllllllllllld",
            ".dlllsllllllsllld.",
            ".dllsssllllsssld..",
            "..dllsssssssslld..",
            "...ddllsssllldd...",
            ".....dddddddd.....",
            ".......dbd........",
            ".......dbd........",
            ".......dbd........",
            "....ssssssssss....",
        ],
        &[
            ('d', Color::rgb(38, 62, 59)),
            ('l', Color::rgb(57, 123, 80)),
            ('h', Color::rgb(142, 188, 101)),
            ('s', Color::rgb(70, 110, 73)),
            ('b', Color::rgb(151, 133, 93)),
        ],
    );
    let flowers = pixel_art(
        assets,
        &[".y....", "yyy...", ".hl.y.", ".l.yyy", "....l."],
        &[
            ('y', Color::rgb(239, 207, 124)),
            ('h', Color::rgb(255, 242, 177)),
            ('l', Color::rgb(57, 123, 80)),
        ],
    );
    let rock = pixel_art(
        assets,
        &["..hh..", ".hsss.", ".sssd.", "dddddd"],
        &[
            ('h', Color::rgb(181, 186, 145)),
            ('s', Color::rgb(121, 139, 122)),
            ('d', Color::rgb(70, 110, 73)),
        ],
    );
    let player = assets.insert(player_image.clone());
    let shard = pixel_art(
        assets,
        &[
            "...h....", "..dhds..", ".dhggdss", ".dhggds.", "..dgd...", "...d....", "........",
            "..ssss..",
        ],
        &[
            ('d', Color::rgb(42, 115, 120)),
            ('h', Color::rgb(232, 255, 255)),
            ('g', Color::rgb(87, 219, 220)),
            ('s', Color::rgb(70, 110, 73)),
        ],
    );
    let shrine = |assets: &mut ImageAssets, active| {
        pixel_art(
            assets,
            &[
                "......hh......",
                ".....dggd.....",
                "......dd......",
                "..............",
                "..dddddddddd..",
                "..dhhhhhhmhd..",
                "...dssssmmd...",
                "...dsddddsd...",
                "...dsdggdsd...",
                "...dsdggdsd...",
                "...dsddddsd...",
                "...dmsssssd...",
                "..dmmssssssd..",
                ".dhhhhhhhhhhd.",
                ".dddddddddddd.",
                "..ssssssssss..",
            ],
            &[
                ('d', Color::rgb(55, 76, 65)),
                (
                    'h',
                    if active {
                        Color::rgb(229, 214, 143)
                    } else {
                        Color::rgb(181, 186, 145)
                    },
                ),
                ('s', Color::rgb(121, 139, 122)),
                ('m', Color::rgb(57, 123, 80)),
                (
                    'g',
                    if active {
                        Color::rgb(87, 219, 220)
                    } else {
                        Color::rgb(75, 106, 99)
                    },
                ),
            ],
        )
    };
    Art {
        meadow,
        tree,
        flowers,
        rock,
        player,
        shard,
        shrine_inactive: shrine(assets, false),
        shrine_active: shrine(assets, true),
    }
}

// A continuous, softly stepped trail follows the actual eleven-tick shard route.
// It is visual guidance only: movement remains legal across the entire map.
fn meadow_path(x: i32, y: i32) -> bool {
    ((12..=40).contains(&x) && (15..=25).contains(&y))
        || ((31..=41).contains(&x) && (20..=48).contains(&y))
        || ((36..=90).contains(&x) && (39..=49).contains(&y))
        || ((76..=92).contains(&x) && (28..=49).contains(&y))
        || ((78..=90).contains(&x) && (26..=51).contains(&y))
}

pub fn generated_player() -> Image {
    pixel_image(
        &[
            "..dddd..", "..dhhhd.", "..dfffd.", "..ffdf..", ".ddccdd.", ".dchccd.", "dcchcccd",
            ".dcccdd.", "..dddd..", "..d..d..",
        ],
        &[
            ('d', Color::rgb(62, 48, 47)),
            ('h', Color::rgb(181, 154, 110)),
            ('f', Color::rgb(237, 207, 145)),
            ('c', Color::rgb(201, 101, 72)),
        ],
    )
}

fn pixel_art(assets: &mut ImageAssets, rows: &[&str], palette: &[(char, Color)]) -> ImageId {
    assets.insert(pixel_image(rows, palette))
}

fn pixel_image(rows: &[&str], palette: &[(char, Color)]) -> Image {
    let width = rows[0].len();
    assert!(rows.iter().all(|row| row.len() == width));
    Image::from_fn(width as u32, rows.len() as u32, |x, y| {
        let pixel = rows[y as usize].as_bytes()[x as usize] as char;
        palette
            .iter()
            .find_map(|(key, color)| (*key == pixel).then_some(*color))
            .unwrap_or(Color::TRANSPARENT)
    })
    .unwrap()
}

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
    fn position_fields_require_opt_in_and_validate_before_mutation() {
        use titan::inspection::InspectionConfig;
        use titan_protocol::{ErrorCode, Request, Response, ResponseOutcome};
        let mut app = build_game();
        app.update_schedule(titan::Startup);
        let player = app.world().iter::<super::Player>().next().unwrap().0;
        let entity = titan_protocol::EntityId {
            index: player.index(),
            generation: player.generation(),
        };
        let component = std::any::type_name::<super::Position>().to_owned();
        let edit = |value| Request::SetField {
            entity,
            component: component.clone(),
            field: "x".into(),
            value,
        };
        let mut inspector = super::build_inspector("unused.ppm".into());
        let denied = request(&mut app, &mut inspector, edit(3.into()));
        assert!(
            matches!(denied.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::MutationDisabled)
        );
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 2);
        let mut config = InspectionConfig::controlled("editable-rpg", "test");
        config.mutation_enabled = true;
        let mut inspector = super::configured_inspector("unused.ppm".into(), config);
        success(request(&mut app, &mut inspector, edit(3.into())));
        for value in [20.into(), (-1).into(), "3".into()] {
            let rejected = request(&mut app, &mut inspector, edit(value));
            assert_eq!(rejected.state_revision, 1);
            assert_eq!(rejected.observed_frame, 0);
            assert!(
                matches!(rejected.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::InvalidValue)
            );
        }
        let Response::Entity(details) = success(request(
            &mut app,
            &mut inspector,
            Request::Entity { entity },
        )) else {
            panic!("expected entity")
        };
        assert_eq!(details.components[&component]["x"], 3);
        let expected = [("x", 19.0), ("y", 13.0)]
            .into_iter()
            .map(|(name, maximum)| {
                (
                    name.into(),
                    titan_protocol::FieldMetadata {
                        type_name: "i32".into(),
                        description: "Map tile coordinate".into(),
                        writable: true,
                        minimum: Some(0.0),
                        maximum: Some(maximum),
                        unit: Some("tile".into()),
                    },
                )
            })
            .collect();
        assert_eq!(details.component_fields[&component], expected);
    }

    #[test]
    fn overlapping_shards_are_collected_once_at_the_schedule_boundary() {
        let mut app = build_game();
        app.update_schedule(titan::Startup);
        for _ in 0..3 {
            super::spawn_at(
                app.world_mut(),
                super::Position { x: 2, y: 2 },
                super::Shard,
                "overlapping-shard",
            );
        }
        app.try_advance_fixed(1).unwrap();
        assert_eq!(
            app.world()
                .resource::<QuestState>()
                .unwrap()
                .collected_shards,
            3
        );
        assert_eq!(app.world().iter::<super::Shard>().count(), 3);
        assert_eq!(app.world().iter::<super::ActiveShrine>().count(), 1);
        app.try_advance_fixed(1).unwrap();
        assert_eq!(
            app.world()
                .resource::<QuestState>()
                .unwrap()
                .collected_shards,
            3
        );
        // Snapshot hooks require exclusive access; gameplay systems remain typed.
        assert!(
            app.system_metadata(titan::FixedUpdate)
                .filter(|system| !system.name.contains("::live::"))
                .all(|system| {
                    system
                        .accesses
                        .iter()
                        .all(|access| access.target != titan::AccessTarget::World)
                })
        );
    }

    #[test]
    fn interactive_movement_repeats_every_six_ticks_and_releases() {
        let mut app = build_game();
        let mut input = super::InteractiveInput::default();
        input.set_action("right", true).unwrap();
        input.tick(&mut app);
        let player = app.world().iter::<super::Player>().next().unwrap().0;
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 3);
        for _ in 0..5 {
            input.tick(&mut app);
        }
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 3);
        input.tick(&mut app);
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 4);
        input.set_action("right", false).unwrap();
        for _ in 0..12 {
            input.tick(&mut app);
        }
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 4);
        assert!(input.set_action("jump", true).is_err());
    }

    #[test]
    fn interactive_taps_survive_release_before_the_next_fixed_tick() {
        let mut app = build_game();
        let mut input = super::InteractiveInput::default();
        input.set_action("right", true).unwrap();
        input.set_action("right", false).unwrap();
        input.tick(&mut app);
        let player = app.world().iter::<super::Player>().next().unwrap().0;
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 3);
        input.tick(&mut app);
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 3);
        for _ in 0..12 {
            input.tick(&mut app);
        }
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 3);
    }

    #[test]
    fn interactive_cancellation_discards_a_pending_tap() {
        let mut app = build_game();
        let mut input = super::InteractiveInput::default();
        input.set_action("right", true).unwrap();
        input.set_action("right", false).unwrap();
        input.clear();
        input.tick(&mut app);
        let player = app.world().iter::<super::Player>().next().unwrap().0;
        assert_eq!(app.world().get::<super::Position>(player).unwrap().x, 2);
    }

    #[test]
    fn canceling_one_direction_preserves_another_pending_tap() {
        let mut app = build_game();
        let mut input = super::InteractiveInput::default();
        for direction in ["right", "down"] {
            input.set_action(direction, true).unwrap();
            input.set_action(direction, false).unwrap();
        }
        input.cancel_action("down").unwrap();
        input.tick(&mut app);
        let player = app.world().iter::<super::Player>().next().unwrap().0;
        let position = app.world().get::<super::Position>(player).unwrap();
        assert_eq!((position.x, position.y), (3, 2));
    }

    #[test]
    fn recorded_walk_collects_every_shard_and_activates_the_shrine() {
        let mut app = build_game();
        let recording = recorded_walk();
        replay(&mut app, &recording);

        let quest = app.world().resource::<QuestState>().unwrap();
        assert_eq!(quest.collected_shards, 3);
        assert!(quest.shrine_active);
        assert_eq!(recording.len(), 11);

        let frame = app.extracted::<titan::render::RenderFrame>().unwrap();
        let image = SoftwareRenderer::render(frame, app.world().resource::<ImageAssets>().unwrap())
            .unwrap();
        assert_eq!(image_checksum(&image), 0xf7a2_98f6_2ad7_5c1c);
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
        assert_eq!(page.entities.len(), 11);
        let hud = page
            .entities
            .iter()
            .find(|entity| entity.name.as_deref() == Some("ui/quest"))
            .unwrap()
            .id;
        let Response::Entity(hud_details) = success(request(
            &mut app,
            &mut inspector,
            Request::Entity { entity: hud },
        )) else {
            panic!("expected UI entity")
        };
        let ui_text = std::any::type_name::<titan::ui::UiText>();
        assert_eq!(hud_details.components[ui_text]["text"], "SHARDS 0/3");
        assert!(!hud_details.component_fields[ui_text]["text"].writable);
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
        let Response::Entity(hud_details) = success(request(
            &mut app,
            &mut inspector,
            Request::Entity { entity: hud },
        )) else {
            panic!("expected updated UI entity")
        };
        assert_eq!(
            hud_details.components[ui_text]["text"],
            "SHARDS 3/3  SHRINE ACTIVE"
        );
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
        assert_eq!(page.entities.len(), 8);
        let Response::Capture(capture) =
            success(request(&mut app, &mut inspector, Request::Capture))
        else {
            panic!("expected capture")
        };
        assert_eq!((capture.width, capture.height), (160, 112));
        assert_eq!(capture.checksum, "f7a298f62ad75c1c");
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
        assert_eq!(app.world().entities().count(), 11);
        assert!(
            !app.world()
                .resource::<super::ScheduledInput>()
                .unwrap()
                .enabled
        );
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

/// Converts held interactive directions into repeatable tile movement pulses.
/// Protocol recordings bypass this helper and keep their exact per-tick input.
#[derive(Default)]
pub struct InteractiveInput {
    buttons: BufferedButtons<Action>,
    tracker: InputTracker<Action>,
    repeat_in: u8,
}

impl InteractiveInput {
    pub fn clear(&mut self) {
        self.buttons.clear();
        self.tracker = InputTracker::default();
        self.repeat_in = 0;
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
        self.repeat_in = 0;
        Ok(())
    }

    pub fn set_action(&mut self, name: &str, pressed: bool) -> Result<(), String> {
        let action = Self::action(name)?;
        if self.buttons.set(action, pressed, true) {
            self.repeat_in = 0;
        }
        Ok(())
    }

    pub fn tick(&mut self, app: &mut App) {
        let values = if self.repeat_in == 0 {
            self.repeat_in = 5;
            let presses = self.buttons.take_presses();
            self.buttons
                .held()
                .union(&presses)
                .copied()
                .map(|action| (action, ActionValue::PRESSED))
                .collect::<Vec<_>>()
        } else {
            self.repeat_in -= 1;
            Vec::new()
        };
        app.world_mut().insert_resource(self.tracker.sample(values));
        app.advance_fixed(1);
    }
}

pub fn status(app: &App) -> String {
    let quest = app.world().resource::<QuestState>().unwrap();
    serde_json::json!({
        "frame": app.world().resource::<FixedTime>().unwrap().tick(),
        "collected_shards": quest.collected_shards,
        "shrine_active": quest.shrine_active,
    })
    .to_string()
}
