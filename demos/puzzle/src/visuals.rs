//! Minimal read-only view. Issue #80 owns this module and future animation/art.

use alloc::{collections::BTreeSet, format, string::String};

use bevy::{prelude::*, window::PrimaryWindow};
use titan_puzzle::{level::Cell, simulation::EventKind, Game, GameplayEvent};

/// Read-only board and HUD presentation; input belongs to the UX adapter.
pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, present);
    }
}

/// A shape's stable simulation binding and size at the nominal 52-pixel tile.
#[derive(Component)]
struct BoardVisual {
    binding: VisualBinding,
    nominal_size: Vec2,
}

enum VisualBinding {
    Cell(Cell),
    TargetCoverage(Cell),
    Block(String),
    Player,
}

impl BoardVisual {
    fn update(
        &self,
        game: &Game,
        layout: &BoardLayout,
        occupied_targets: &BTreeSet<Cell>,
        sprite: &mut Sprite,
        transform: &mut Transform,
        visibility: &mut Visibility,
    ) {
        let cell = match &self.binding {
            VisualBinding::Cell(cell) | VisualBinding::TargetCoverage(cell) => *cell,
            VisualBinding::Block(id) => game.state().blocks()[id],
            VisualBinding::Player => game.state().player(),
        };
        let position = layout.position(cell);
        sprite.custom_size = Some(self.nominal_size * (layout.tile / 52.0));
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        *visibility = if matches!(self.binding, VisualBinding::TargetCoverage(_))
            && !occupied_targets.contains(&cell)
        {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

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

fn spawn_board(
    commands: &mut Commands,
    game: &Game,
    layout: &BoardLayout,
    occupied_targets: &BTreeSet<Cell>,
) {
    let mut shape = |binding, z, size, color, angle| {
        let visual = BoardVisual {
            binding,
            nominal_size: size,
        };
        let mut sprite = Sprite::from_color(color, size);
        let mut transform =
            Transform::from_xyz(0.0, 0.0, z).with_rotation(Quat::from_rotation_z(angle));
        let mut visibility = Visibility::Inherited;
        visual.update(
            game,
            layout,
            occupied_targets,
            &mut sprite,
            &mut transform,
            &mut visibility,
        );
        commands.spawn((sprite, transform, visibility, visual));
    };
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
                VisualBinding::Cell(cell),
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
                VisualBinding::Cell(target.position),
                1.0,
                size,
                Color::srgb(0.5, 0.89, 0.78),
                0.0,
            );
        }
        // Keep a marker for every target, including currently uncovered ones.
        shape(
            VisualBinding::TargetCoverage(target.position),
            4.0,
            Vec2::splat(10.0),
            Color::srgb(0.5, 0.89, 0.78),
            0.0,
        );
    }
    for id in game.state().blocks().keys() {
        for (z, size, color) in [
            (2.0, 37.0, Color::srgb(0.94, 0.68, 0.33)),
            (3.0, 21.0, Color::srgb(0.37, 0.26, 0.16)),
        ] {
            shape(
                VisualBinding::Block(id.clone()),
                z,
                Vec2::splat(size),
                color,
                0.0,
            );
        }
    }
    shape(
        VisualBinding::Player,
        5.0,
        Vec2::splat(24.0),
        Color::srgb(0.88, 0.93, 1.0),
        core::f32::consts::FRAC_PI_4,
    );
}

fn present(
    mut commands: Commands,
    game: Res<Game>,
    mut events: MessageReader<GameplayEvent>,
    mut visuals: Query<(
        Entity,
        &BoardVisual,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut drawn_level: Local<Option<String>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut previous_viewport: Local<Option<Vec2>>,
) {
    // This reader is independent of audio/UX. State remains authoritative when
    // rendering starts late or a consumer misses Bevy's short-lived messages.
    let mut changed = false;
    // Do not use Iterator::any: it would leave later facts unread this update.
    for event in events.read() {
        changed |= !matches!(
            event.kind,
            EventKind::Blocked { .. } | EventKind::Ignored(_)
        );
    }
    let viewport = Vec2::new(window.width(), window.height());
    let level = game.level();
    let level_changed = drawn_level.as_deref() != Some(level.id());
    if !level_changed && !changed && *previous_viewport == Some(viewport) {
        return;
    }
    *previous_viewport = Some(viewport);
    let layout = BoardLayout::new(viewport, level.width(), level.height());
    // Build occupancy once, rather than scanning every target for each block.
    let block_cells: BTreeSet<_> = game.state().blocks().values().copied().collect();
    let occupied_targets = level
        .targets()
        .iter()
        .map(|target| target.position)
        .filter(|cell| block_cells.contains(cell))
        .collect();
    if level_changed {
        for (entity, ..) in &mut visuals {
            commands.entity(entity).despawn();
        }
        spawn_board(&mut commands, &game, &layout, &occupied_targets);
        *drawn_level = Some(level.id().into());
    } else {
        for (_, visual, mut sprite, mut transform, mut visibility) in &mut visuals {
            visual.update(
                &game,
                &layout,
                &occupied_targets,
                &mut sprite,
                &mut transform,
                &mut visibility,
            );
        }
    }
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
    use alloc::{collections::BTreeMap, vec, vec::Vec};

    use super::*;
    use titan_puzzle::{level::Level, simulation::Direction, Action};

    const LEVEL: &str = "puzzle 1\nid presentation\ntitle Presentation\ngrid\n#####\n#...#\n#...#\n#####\nobjects\nplayer hero 1 2\nblock crate 2 1\ntarget goal 3 1\nend\n";

    type Snapshot = BTreeMap<Entity, (Transform, Option<Vec2>, Visibility)>;

    fn app(levels: Vec<Level>) -> (App, Entity) {
        // Only presentation systems and messages: no renderer, window backend,
        // asset server, input, or other engine plugins are needed.
        let mut app = App::new();
        app.insert_resource(Game::new(levels).unwrap())
            .add_message::<GameplayEvent>()
            .add_plugins(VisualsPlugin);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (960, 600).into(),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        (app, window)
    }

    fn action(app: &mut App, action: Action) {
        let events = app.world_mut().resource_mut::<Game>().step(action);
        app.world_mut()
            .resource_mut::<Messages<GameplayEvent>>()
            .write_batch(events);
        app.world_mut().run_schedule(Update);
    }

    fn snapshot(app: &mut App) -> Snapshot {
        app.world_mut()
            .query_filtered::<(Entity, &Transform, &Sprite, &Visibility), With<BoardVisual>>()
            .iter(app.world())
            .map(|(entity, transform, sprite, visibility)| {
                (entity, (*transform, sprite.custom_size, *visibility))
            })
            .collect()
    }

    fn hud(app: &mut App) -> (Entity, String) {
        let (entity, text) = app
            .world_mut()
            .query_filtered::<(Entity, &Text), With<Hud>>()
            .single(app.world())
            .unwrap();
        (entity, text.0.clone())
    }

    fn assert_idle(app: &mut App) {
        let (entity, text) = hud(app);
        // Pending unread redraw facts would overwrite this on the next update.
        app.world_mut().get_mut::<Text>(entity).unwrap().0 = "idle sentinel".into();
        app.world_mut().run_schedule(Update);
        assert_eq!(hud(app), (entity, "idle sentinel".into()));
        app.world_mut().get_mut::<Text>(entity).unwrap().0 = text;
    }

    fn assert_noop(app: &mut App, command: Action) {
        let before = snapshot(app);
        let (entity, text) = hud(app);
        app.world_mut().get_mut::<Text>(entity).unwrap().0 = "noop sentinel".into();
        action(app, command);
        assert_eq!(snapshot(app), before);
        assert_eq!(hud(app), (entity, "noop sentinel".into()));
        app.world_mut().get_mut::<Text>(entity).unwrap().0 = text;
    }

    #[test]
    fn presentation_retains_entities_across_actions_and_resize() {
        let (mut app, window) = app(vec![Level::parse("test", LEVEL).unwrap()]);
        let initial = snapshot(&mut app);
        // Terrain, target cross/coverage, block inset, and player all exist on
        // the first draw, even though no gameplay event has been sent yet.
        assert_eq!(initial.len(), 5 * 4 + 3 + 2 + 1);
        let initial_hud = hud(&mut app);
        assert!(initial_hud.1.contains("Moves: 0"));
        assert_idle(&mut app);

        assert_noop(&mut app, Action::Move(Direction::Down)); // Boundary wall.
        assert_noop(&mut app, Action::Undo); // No history yet.
        assert_eq!(snapshot(&mut app), initial);
        assert_eq!(hud(&mut app), initial_hud);
        assert_idle(&mut app);

        action(&mut app, Action::Move(Direction::Up)); // Walk without pushing.
        let walked = snapshot(&mut app);
        assert_eq!(
            walked.keys().collect::<Vec<_>>(),
            initial.keys().collect::<Vec<_>>()
        );
        assert_ne!(walked, initial);
        assert!(hud(&mut app).1.contains("Moves: 1"));

        action(&mut app, Action::Move(Direction::Right)); // Cover the target.
        let pushed = snapshot(&mut app);
        assert_eq!(
            pushed.keys().collect::<Vec<_>>(),
            initial.keys().collect::<Vec<_>>()
        );
        assert_ne!(pushed, walked);
        assert!(hud(&mut app).1.contains("Moves: 2"));
        let marker = app
            .world_mut()
            .query::<(Entity, &BoardVisual)>()
            .iter(app.world())
            .find(|(_, visual)| matches!(visual.binding, VisualBinding::TargetCoverage(_)))
            .unwrap()
            .0;
        assert_eq!(initial[&marker].2, Visibility::Hidden);
        assert_eq!(pushed[&marker].2, Visibility::Inherited);
        // The push emits several facts; every one must be drained this update.
        assert_idle(&mut app);
        let solved_hud = hud(&mut app);
        assert_noop(&mut app, Action::Move(Direction::Right)); // Completed-level block.
        assert_noop(&mut app, Action::NextLevel); // Last-level no-op.
        assert_eq!(snapshot(&mut app), pushed);
        assert_eq!(hud(&mut app), solved_hud);
        assert_idle(&mut app);

        action(&mut app, Action::Undo);
        assert_eq!(snapshot(&mut app), walked);
        action(&mut app, Action::Undo);
        assert_eq!(snapshot(&mut app), initial);
        action(&mut app, Action::Redo);
        assert_eq!(snapshot(&mut app), walked);
        action(&mut app, Action::Restart);
        assert_eq!(snapshot(&mut app), initial);
        assert_eq!(hud(&mut app), initial_hud);

        let viewport = Vec2::new(160.0, 120.0);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(viewport.x, viewport.y);
        app.world_mut().run_schedule(Update);
        let resized = snapshot(&mut app);
        assert_eq!(
            resized.keys().collect::<Vec<_>>(),
            initial.keys().collect::<Vec<_>>()
        );
        assert_eq!(hud(&mut app).0, initial_hud.0);
        let game = app.world().resource::<Game>();
        let layout = BoardLayout::new(viewport, game.level().width(), game.level().height());
        for (entity, visual) in app
            .world_mut()
            .query::<(Entity, &BoardVisual)>()
            .iter(app.world())
        {
            let cell = match &visual.binding {
                VisualBinding::Cell(cell) | VisualBinding::TargetCoverage(cell) => *cell,
                VisualBinding::Block(_) => Cell::new(2, 1),
                VisualBinding::Player => Cell::new(1, 2),
            };
            let (transform, size, visibility) = &resized[&entity];
            assert_eq!(transform.translation.truncate(), layout.position(cell));
            assert_eq!(*size, Some(visual.nominal_size * (layout.tile / 52.0)));
            assert_ne!(*size, initial[&entity].1);
            assert_eq!(transform.translation.z, initial[&entity].0.translation.z);
            assert_eq!(transform.rotation, initial[&entity].0.rotation);
            assert_eq!(*visibility, initial[&entity].2);
        }
        assert_idle(&mut app);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(960.0, 600.0);
        app.world_mut().run_schedule(Update);
        assert_eq!(snapshot(&mut app), initial);
    }

    #[test]
    fn only_a_new_level_id_rebuilds_the_board() {
        let levels = vec![
            Level::parse("first", LEVEL).unwrap(),
            Level::parse("second", &LEVEL.replace("id presentation", "id second")).unwrap(),
        ];
        let (mut app, _) = app(levels);
        let initial = snapshot(&mut app);
        let hud_entity = hud(&mut app).0;
        action(&mut app, Action::Move(Direction::Up));
        action(&mut app, Action::Move(Direction::Right));
        action(&mut app, Action::NextLevel);
        let next = snapshot(&mut app);
        assert_eq!(next.len(), initial.len());
        assert!(next.keys().all(|entity| !initial.contains_key(entity)));
        assert_eq!(hud(&mut app).0, hud_entity);
        assert!(hud(&mut app).1.contains("2 / 2"));
        assert_idle(&mut app);
    }

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
