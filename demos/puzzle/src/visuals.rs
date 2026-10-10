//! Minimal read-only view. Issue #80 owns this module and future animation/art.

use bevy::prelude::*;
use titan_puzzle::{level::Cell, Game, GameplayEvent};

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, present);
    }
}

#[derive(Component)]
struct BoardVisual;
#[derive(Component)]
struct Hud;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(21.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.93, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            top: px(24),
            left: px(28),
            ..default()
        },
        Hud,
    ));
}

fn shape(
    commands: &mut Commands,
    game: &Game,
    cell: Cell,
    z: f32,
    size: Vec2,
    color: Color,
    angle: f32,
) {
    let scale = 52.0;
    let x = (cell.x as f32 - (game.level().width() as f32 - 1.0) / 2.0) * scale;
    let y = ((game.level().height() as f32 - 1.0) / 2.0 - cell.y as f32) * scale - 15.0;
    commands.spawn((
        Sprite::from_color(color, size),
        Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_z(angle)),
        BoardVisual,
    ));
}

fn present(
    mut commands: Commands,
    game: Res<Game>,
    mut events: MessageReader<GameplayEvent>,
    visuals: Query<Entity, With<BoardVisual>>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut drawn: Local<bool>,
) {
    // This reader is independent of audio/UX. State remains authoritative when
    // rendering starts late or a consumer misses Bevy's short-lived messages.
    let changed = events.read().count() > 0;
    if *drawn && !changed {
        return;
    }
    *drawn = true;
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    let level = game.level();
    for y in 0..level.height() {
        for x in 0..level.width() {
            let cell = Cell::new(x as i32, y as i32);
            let color = if level.is_wall(cell) {
                Color::srgb(0.29, 0.36, 0.43)
            } else {
                Color::srgb(0.12, 0.18, 0.23)
            };
            shape(
                &mut commands,
                &game,
                cell,
                0.0,
                Vec2::splat(49.0),
                color,
                0.0,
            );
        }
    }
    // Targets use a visible cross, blocks an inset square, player a diamond:
    // object roles remain recognizable without relying on colour alone.
    for target in level.targets() {
        for size in [Vec2::new(30.0, 7.0), Vec2::new(7.0, 30.0)] {
            shape(
                &mut commands,
                &game,
                target.position,
                1.0,
                size,
                Color::srgb(0.5, 0.89, 0.78),
                0.0,
            );
        }
    }
    for cell in game.state().blocks().values() {
        shape(
            &mut commands,
            &game,
            *cell,
            2.0,
            Vec2::splat(37.0),
            Color::srgb(0.94, 0.68, 0.33),
            0.0,
        );
        shape(
            &mut commands,
            &game,
            *cell,
            3.0,
            Vec2::splat(21.0),
            Color::srgb(0.37, 0.26, 0.16),
            0.0,
        );
        if level
            .targets()
            .iter()
            .any(|target| target.position == *cell)
        {
            shape(
                &mut commands,
                &game,
                *cell,
                4.0,
                Vec2::splat(10.0),
                Color::srgb(0.5, 0.89, 0.78),
                0.0,
            );
        }
    }
    shape(
        &mut commands,
        &game,
        game.state().player(),
        5.0,
        Vec2::splat(24.0),
        Color::srgb(0.88, 0.93, 1.0),
        core::f32::consts::FRAC_PI_4,
    );
    let status = if game.state().complete() {
        if game.level_index() + 1 == game.level_count() {
            "All starter levels solved! Z: undo / R: replay"
        } else {
            "Solved! Advancing shortly (Z: undo)"
        }
    } else {
        "Arrows / WASD: move    Z: undo    Y: redo    R: restart"
    };
    hud.0 = format!(
        "TITAN / PUZZLE     {} / {} - {}\nMoves: {}     {}",
        game.level_index() + 1,
        game.level_count(),
        level.title(),
        game.state().moves(),
        status
    );
}
