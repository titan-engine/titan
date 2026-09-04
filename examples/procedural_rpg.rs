use std::fs;
use std::path::{Path, PathBuf};

use titan::input::{ActionValue, InputFrame, InputRecording, InputTracker, RecordingHeader};
use titan::render::{
    Color, Image, ImageAssets, ImageId, RenderFrame, SoftwareRenderer, SpriteDraw,
};
use titan::{App, Component, FixedUpdate, Startup, World};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Default)]
struct QuestState {
    collected_shards: usize,
    shrine_active: bool,
}

#[derive(Clone, Copy)]
struct Art {
    grass: [ImageId; 2],
    player: ImageId,
    shard: ImageId,
    shrine_inactive: ImageId,
    shrine_active: ImageId,
}

struct ExtractedFrame(RenderFrame);

fn main() {
    let mut app = build_game();

    replay(&mut app, &recorded_walk());

    let frame = &app
        .world()
        .resource::<ExtractedFrame>()
        .expect("the fixed update extracts a frame")
        .0;
    let assets = app.world().resource::<ImageAssets>().unwrap();
    let image = SoftwareRenderer::render(frame, assets).unwrap();
    let output_path = PathBuf::from("target/titan/procedural-rpg.ppm");
    write_ppm(&output_path, &image);

    let quest = app.world().resource::<QuestState>().unwrap();
    println!(
        "wrote {} ({} shards, shrine active: {}, checksum: {:016x})",
        output_path.display(),
        quest.collected_shards,
        quest.shrine_active,
        image_checksum(&image)
    );
}

fn build_game() -> App {
    let mut app = App::new();
    app.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    app.world_mut().insert_resource(QuestState::default());
    app.add_systems(Startup, setup);
    app.add_systems(FixedUpdate, move_player);
    app.add_systems(FixedUpdate, collect_shards);
    app.add_systems(FixedUpdate, extract_frame);
    app
}

fn setup(world: &mut World) {
    let mut assets = ImageAssets::new();
    let art = generate_art(&mut assets);
    world.insert_resource(assets);
    world.insert_resource(art);

    spawn_at(world, Position { x: 2, y: 2 }, Player);
    spawn_at(world, Position { x: 4, y: 2 }, Shard);
    spawn_at(world, Position { x: 4, y: 5 }, Shard);
    spawn_at(world, Position { x: 8, y: 5 }, Shard);
    spawn_at(world, Position { x: 10, y: 5 }, Shrine);
}

fn spawn_at<T: Component>(world: &mut World, position: Position, marker: T) {
    let entity = world.spawn();
    world.insert(entity, position).unwrap();
    world.insert(entity, marker).unwrap();
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
    state.shrine_active = state.collected_shards == 3;
    let mut commands = world.commands();
    for shard in collected {
        commands.despawn(shard);
    }
}

fn extract_frame(world: &mut World) {
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

    world.insert_resource(ExtractedFrame(frame));
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

fn recorded_walk() -> InputRecording<Action> {
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

fn replay(app: &mut App, recording: &InputRecording<Action>) {
    for frame in recording.frames() {
        *app.world_mut()
            .resource_mut::<InputFrame<Action>>()
            .unwrap() = frame.clone();
        app.advance_fixed(1);
    }
}

fn write_ppm(path: &Path, image: &Image) {
    let mut bytes = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
    let (pixels, remainder) = image.pixels().as_chunks::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        bytes.extend_from_slice(&pixel[..3]);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

fn image_checksum(image: &Image) -> u64 {
    image
        .pixels()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        })
}

#[cfg(test)]
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
}
