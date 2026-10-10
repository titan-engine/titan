//! Minimal read-only view. Issue #80 owns this module and future animation/art.

use bevy::{prelude::*, window::PrimaryWindow};
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

/// Layout in logical pixels, matching Camera2d's default orthographic viewport.
struct BoardLayout {
    tile: f32,
    center: Vec2,
    dimensions: Vec2,
}

impl BoardLayout {
    fn new(viewport: Vec2, width: usize, height: usize) -> Self {
        let dimensions = Vec2::new(width as f32, height as f32);
        let top = (viewport.y * 0.4).min(140.0);
        let bottom = (viewport.y * 0.05).min(28.0);
        let side = (viewport.x * 0.05).min(28.0);
        let available = Vec2::new(viewport.x - side * 2.0, viewport.y - top - bottom);
        Self {
            tile: 52.0_f32
                .min(available.x / dimensions.x)
                .min(available.y / dimensions.y),
            center: Vec2::new(0.0, (bottom - top) / 2.0),
            dimensions,
        }
    }

    fn position(&self, cell: Cell) -> Vec2 {
        self.center
            + Vec2::new(
                cell.x as f32 - (self.dimensions.x - 1.0) / 2.0,
                (self.dimensions.y - 1.0) / 2.0 - cell.y as f32,
            ) * self.tile
    }
}

fn shape(
    commands: &mut Commands,
    layout: &BoardLayout,
    cell: Cell,
    z: f32,
    size: Vec2,
    color: Color,
    angle: f32,
) {
    let position = layout.position(cell);
    commands.spawn((
        Sprite::from_color(color, size * (layout.tile / 52.0)),
        Transform::from_xyz(position.x, position.y, z).with_rotation(Quat::from_rotation_z(angle)),
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
    window: Single<&Window, With<PrimaryWindow>>,
    mut previous_viewport: Local<Option<Vec2>>,
) {
    // This reader is independent of audio/UX. State remains authoritative when
    // rendering starts late or a consumer misses Bevy's short-lived messages.
    let changed = events.read().count() > 0;
    let viewport = Vec2::new(window.width(), window.height());
    if *drawn && !changed && *previous_viewport == Some(viewport) {
        return;
    }
    *drawn = true;
    *previous_viewport = Some(viewport);
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    let level = game.level();
    let layout = BoardLayout::new(viewport, level.width(), level.height());
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
                &layout,
                cell,
                0.0,
                Vec2::splat(49.0),
                color,
                0.0,
            );
        }
    }
    // Targets use a visible cross, blocks an inset square, player a diamond:
    // object roles remain recognizable without relying on color alone.
    for target in level.targets() {
        for size in [Vec2::new(30.0, 7.0), Vec2::new(7.0, 30.0)] {
            shape(
                &mut commands,
                &layout,
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
            &layout,
            *cell,
            2.0,
            Vec2::splat(37.0),
            Color::srgb(0.94, 0.68, 0.33),
            0.0,
        );
        shape(
            &mut commands,
            &layout,
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
                &layout,
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
        &layout,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_boards_fit_below_hud_at_standard_and_resized_viewports() {
        for viewport in [
            Vec2::new(960.0, 600.0),
            Vec2::new(640.0, 480.0),
            Vec2::new(320.0, 240.0),
        ] {
            for (width, height) in [(64, 64), (64, 3), (3, 64), (5, 4)] {
                let layout = BoardLayout::new(viewport, width, height);
                let top = (viewport.y * 0.4).min(140.0);
                let bottom = (viewport.y * 0.05).min(28.0);
                for cell in [
                    Cell::new(0, 0),
                    Cell::new(width as i32 - 1, height as i32 - 1),
                ] {
                    let p = layout.position(cell);
                    let half = layout.tile / 2.0;
                    assert!(p.x - half >= -viewport.x / 2.0);
                    assert!(p.x + half <= viewport.x / 2.0);
                    assert!(p.y - half >= -viewport.y / 2.0 + bottom - 0.001);
                    assert!(p.y + half <= viewport.y / 2.0 - top + 0.001);
                }
            }
        }
    }
}
