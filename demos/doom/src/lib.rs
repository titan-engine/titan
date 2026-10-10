//! Headless, fixed-tick first-person gameplay in an authored grid level.
//!
//! Insert a [`Level`] before adding [`GameplayPlugin`]. A renderer can supply
//! held [`GameplayActions::movement`], accumulate [`GameplayActions::look_delta`],
//! and use the same fire, interaction, and restart actions as headless scenarios.
//! Run Bevy's `FixedUpdate` at [`FIXED_HZ`] (using `Time<Fixed>::from_hz`), or
//! invoke `world.run_schedule(FixedUpdate)` directly in a headless test.
//! No window, renderer, or runtime entity identity is required by this simulation.

mod combat;
pub use combat::{
    CombatState, EnemyState, GamePhase, GameplayEvent, GameplayObject, GameplayOutcome,
    FIRE_COOLDOWN, MAX_AMMO, MAX_HEALTH, SHOT_DAMAGE,
};

use std::collections::HashSet;

use bevy::prelude::*;
use core::{f32::consts::PI, fmt};
use serde::Deserialize;

/// Simulation ticks per second, also used by the renderer's fixed clock.
pub const FIXED_HZ: f64 = 60.0;
/// Player collision radius in world units (one grid cell is one unit).
pub const PLAYER_RADIUS: f32 = 0.2;
/// Walking speed in world units per second, independent of input direction.
pub const MOVE_SPEED: f32 = 3.0;

const MAX_DIMENSION: usize = 128;
const MAX_OBJECTS: usize = 1024;
const PITCH_LIMIT: f32 = PI / 2.0 - 0.01;

/// A supported non-geometric object authored separately from the grid.
#[derive(Clone, Copy, Debug, Deserialize, Reflect, PartialEq, Eq)]
pub enum ObjectKind {
    /// The unique initial player pose.
    Spawn,
    /// A passive landmark; it has no collision or gameplay interaction.
    Marker,
    /// A simple chasing billboard opponent.
    Enemy,
    /// A single-use health pack.
    Health,
    /// A single-use ammo pack.
    Ammo,
    /// The reusable red door key.
    RedKey,
    /// A locked, full-cell door; position must be a cell center.
    RedDoor,
    /// A walk-over exit trigger.
    Exit,
}

/// An authored placement with an identity independent of ECS entity allocation.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectPlacement {
    /// Unique, nonempty printable ASCII ID, at most 64 bytes, without whitespace.
    pub id: String,
    /// The role of this placement.
    pub kind: ObjectKind,
    /// World position `(x, z)`; a cell spans `[x, x + 1] × [z, z + 1]`.
    pub position: Vec2,
    /// Heading in radians: zero faces -Z, positive yaw turns left.
    pub yaw: f32,
}

/// Validated, enclosed grid geometry and separate authored object placements.
///
/// RON format: `(rows: ["#####", "#...#", "#####"], objects:
/// [(id: "spawn", kind: Spawn, position: (2.5, 1.5), yaw: 0.0)])`.
/// Only `#` (solid unit wall) and `.` (flat floor) are supported. Dimensions
/// must be 3–128 cells per axis. Exactly one spawn is required, with circular
/// clearance for [`PLAYER_RADIUS`]. All objects must be finite and on floor.
#[derive(Resource, Clone, Debug)]
pub struct Level {
    width: usize,
    height: usize,
    walls: Vec<bool>,
    objects: Vec<ObjectPlacement>,
}

/// A syntax or validation error in an authored level.
#[derive(Debug)]
pub struct LevelError(String);

impl fmt::Display for LevelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LevelError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelFile {
    rows: Vec<String>,
    objects: Vec<PlacementFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementFile {
    id: String,
    kind: ObjectKind,
    position: (f32, f32),
    yaw: f32,
}

impl Level {
    /// Parse RON and validate dimensions, geometry, IDs, poses, and spawn clearance.
    pub fn parse(source: &str) -> Result<Self, LevelError> {
        let file: LevelFile = ron::from_str(source)
            .map_err(|error| LevelError(format!("invalid level RON: {error}")))?;
        let height = file.rows.len();
        let width = file.rows.first().map_or(0, String::len);
        if !(3..=MAX_DIMENSION).contains(&width) || !(3..=MAX_DIMENSION).contains(&height) {
            return Err(LevelError("grid dimensions must be 3..=128".into()));
        }
        let mut walls = Vec::with_capacity(width * height);
        for (z, row) in file.rows.iter().enumerate() {
            if row.len() != width {
                return Err(LevelError(format!("row {z} has inconsistent width")));
            }
            for (x, cell) in row.bytes().enumerate() {
                if !matches!(cell, b'#' | b'.') {
                    return Err(LevelError(format!("unsupported cell at ({x}, {z})")));
                }
                if (x == 0 || z == 0 || x == width - 1 || z == height - 1) && cell != b'#' {
                    return Err(LevelError(format!("open border at ({x}, {z})")));
                }
                walls.push(cell == b'#');
            }
        }
        if file.objects.len() > MAX_OBJECTS {
            return Err(LevelError("too many objects (maximum 1024)".into()));
        }
        let mut ids = HashSet::new();
        let mut objects = Vec::with_capacity(file.objects.len());
        for (index, placement) in file.objects.into_iter().enumerate() {
            if placement.id.is_empty()
                || placement.id.len() > 64
                || !placement.id.bytes().all(|byte| byte.is_ascii_graphic())
            {
                return Err(LevelError(format!(
                    "object {index} has invalid ID {:?}: expected 1..=64 printable ASCII bytes without whitespace",
                    placement.id,
                )));
            }
            if !ids.insert(placement.id.clone()) {
                return Err(LevelError(format!("duplicate object ID: {}", placement.id)));
            }
            let position = Vec2::new(placement.position.0, placement.position.1);
            if !position.is_finite() || !placement.yaw.is_finite() {
                return Err(LevelError(format!("non-finite pose for {}", placement.id)));
            }
            // Bound floats before integer conversion, including huge finite coordinates.
            if position.x < 0.0
                || position.y < 0.0
                || position.x >= width as f32
                || position.y >= height as f32
                || walls[position.y.floor() as usize * width + position.x.floor() as usize]
            {
                return Err(LevelError(format!(
                    "object {} is not on floor",
                    placement.id
                )));
            }
            objects.push(ObjectPlacement {
                id: placement.id,
                kind: placement.kind,
                position,
                yaw: placement.yaw,
            });
        }
        let level = Self {
            width,
            height,
            walls,
            objects,
        };
        let mut spawns = level
            .objects
            .iter()
            .filter(|object| object.kind == ObjectKind::Spawn);
        let Some(spawn) = spawns.next() else {
            return Err(LevelError("exactly one spawn is required".into()));
        };
        if spawns.next().is_some() {
            return Err(LevelError("exactly one spawn is required".into()));
        }
        if level.collides(spawn.position) {
            return Err(LevelError("spawn lacks player-radius clearance".into()));
        }
        combat::validate_objects(&level)?;
        Ok(level)
    }

    /// Load the built-in industrial chambers and connecting corridor.
    pub fn demo() -> Self {
        Self::parse(include_str!("../levels/industrial.ron"))
            .expect("the bundled industrial level must validate")
    }

    /// Width of the grid in cells along the X axis.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height of the grid in cells along the Z axis.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Whether a cell is solid. Everything outside the grid is solid as well.
    pub fn is_wall(&self, x: i32, z: i32) -> bool {
        x < 0
            || z < 0
            || x >= self.width as i32
            || z >= self.height as i32
            || self.walls[z as usize * self.width + x as usize]
    }

    /// Placements in authored order; their string IDs are persistent identities.
    pub fn objects(&self) -> &[ObjectPlacement] {
        &self.objects
    }

    fn collides(&self, position: Vec2) -> bool {
        if !position.is_finite()
            || position.x < PLAYER_RADIUS
            || position.y < PLAYER_RADIUS
            || position.x > self.width as f32 - PLAYER_RADIUS
            || position.y > self.height as f32 - PLAYER_RADIUS
        {
            return true;
        }
        let min = (position - Vec2::splat(PLAYER_RADIUS)).floor().as_ivec2();
        let max = (position + Vec2::splat(PLAYER_RADIUS)).floor().as_ivec2();
        for z in min.y..=max.y {
            for x in min.x..=max.x {
                if self.is_wall(x, z) {
                    let corner = Vec2::new(x as f32, z as f32);
                    let closest = position.clamp(corner, corner + Vec2::ONE);
                    if position.distance_squared(closest) < PLAYER_RADIUS * PLAYER_RADIUS {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn move_player(&self, position: &mut Vec2, displacement: Vec2) {
        // Each axis advances at most half a radius. A one-cell wall cannot be
        // skipped, and the circle cannot cross a corner between sampled poses.
        // Resolve X then Z in a stable order, retaining the free sliding axis.
        let steps = (displacement.abs().max_element() / (PLAYER_RADIUS * 0.5))
            .ceil()
            .max(1.0) as usize;
        let step = displacement / steps as f32;
        for _ in 0..steps {
            for delta in [Vec2::new(step.x, 0.0), Vec2::new(0.0, step.y)] {
                if !self.collides(*position + delta) {
                    *position += delta;
                } else {
                    // Approach contact rather than dropping an entire step;
                    // this avoids visible gaps and speed-dependent clearance.
                    let mut low = 0.0;
                    let mut high = 1.0;
                    for _ in 0..16 {
                        let middle = (low + high) * 0.5;
                        if self.collides(*position + delta * middle) {
                            high = middle;
                        } else {
                            low = middle;
                        }
                    }
                    *position += delta * low;
                }
            }
        }
    }
}

/// Observable player pose, advanced only by the gameplay fixed schedule.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource)]
pub struct PlayerState {
    /// World-space `(x, z)` center of the collision circle.
    pub position: Vec2,
    /// Heading in radians: zero faces -Z, positive yaw turns left, wrapped to ±π.
    pub yaw: f32,
    /// Vertical aim in radians; positive looks up, clamped away from ±π/2.
    pub pitch: f32,
    /// Completed playing ticks since installation or restart; freezes on death/win.
    pub tick: u64,
}

/// Input shared by interactive presentation and headless scenarios.
#[derive(Resource, Default, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct GameplayActions {
    /// Held local movement: X strafes right, Y walks forward. Length is capped at one.
    pub movement: Vec2,
    /// Accumulated `(yaw, pitch)` radians, consumed once at the next fixed tick.
    pub look_delta: Vec2,
    /// Held trigger; fires whenever the weapon's cooldown expires.
    pub fire: bool,
    /// Pending door interaction, consumed once by the next fixed tick.
    pub interact: bool,
    /// Pending clean restart, consumed once by the next fixed tick.
    pub restart: bool,
}

/// A deterministic snapshot without renderer or ECS entity identities.
#[derive(Clone, Debug, PartialEq)]
pub struct GameplayObservation {
    /// Player pose and completed tick count.
    pub player: PlayerState,
    /// Authored object IDs sorted lexicographically, independent of entity order.
    pub object_ids: Vec<String>,
    /// Gameplay stats, stable object states, and latest tick's structured outcomes.
    pub combat: CombatState,
}

impl GameplayObservation {
    /// Observe an app with [`GameplayPlugin`] installed and a [`Level`] inserted.
    pub fn capture(world: &World) -> Self {
        let mut object_ids: Vec<_> = world
            .resource::<Level>()
            .objects()
            .iter()
            .map(|object| object.id.clone())
            .collect();
        object_ids.sort();
        Self {
            player: world.resource::<PlayerState>().clone(),
            object_ids,
            combat: world.resource::<CombatState>().clone(),
        }
    }
}

/// Install the headless simulation; a validated [`Level`] must already be inserted.
///
/// Initializes [`PlayerState`] and [`CombatState`] from the authored level and
/// installs gameplay in `FixedUpdate`. Does not install window, render, or time
/// plugins. Use [`GameplayActions::restart`] for a clean reset; replacing the
/// level alone does not reset gameplay.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        let spawn = app
            .world()
            .resource::<Level>()
            .objects()
            .iter()
            .find(|object| object.kind == ObjectKind::Spawn)
            .expect("validated level has exactly one spawn");
        let player = PlayerState {
            position: spawn.position,
            yaw: wrap_yaw(spawn.yaw),
            pitch: 0.0,
            tick: 0,
        };
        let combat = CombatState::new(app.world().resource::<Level>());
        app.insert_resource(combat)
            .register_type::<CombatState>()
            .register_type::<GameplayObject>()
            .register_type::<ObjectKind>()
            .register_type::<EnemyState>()
            .register_type::<GamePhase>()
            .register_type::<GameplayEvent>()
            .register_type::<GameplayOutcome>()
            .insert_resource(player)
            .init_resource::<GameplayActions>()
            .register_type::<PlayerState>()
            .register_type::<GameplayActions>()
            .add_systems(FixedUpdate, fixed_gameplay);
    }
}

fn wrap_yaw(yaw: f32) -> f32 {
    (yaw + PI).rem_euclid(2.0 * PI) - PI
}

fn fixed_gameplay(
    level: Res<Level>,
    mut actions: ResMut<GameplayActions>,
    mut player: ResMut<PlayerState>,
    mut combat: ResMut<CombatState>,
) {
    combat.events.clear();
    if core::mem::take(&mut actions.restart) {
        combat::restart(&level, &mut player, &mut actions, &mut combat);
        return;
    }
    let interact = core::mem::take(&mut actions.interact);
    if combat.phase != GamePhase::Playing {
        actions.look_delta = Vec2::ZERO;
        return;
    }
    let look = core::mem::take(&mut actions.look_delta);
    // Ignore invalid input components, and wrap before addition so even finite
    // extreme input cannot overflow or poison the simulation state.
    if look.x.is_finite() {
        player.yaw = wrap_yaw(player.yaw + wrap_yaw(look.x));
    }
    if look.y.is_finite() {
        player.pitch = (player.pitch + look.y.clamp(-PI, PI)).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }
    let mut movement = actions.movement;
    if !movement.is_finite() {
        movement = Vec2::ZERO;
    }
    // Scaling first keeps normalization well-defined even for f32::MAX input.
    movement /= movement.abs().max_element().max(1.0);
    movement = movement.clamp_length_max(1.0);
    let (sin, cos) = ops::sin_cos(player.yaw);
    let right = Vec2::new(cos, -sin);
    let forward = Vec2::new(-sin, -cos);
    let displacement = (right * movement.x + forward * movement.y) * (MOVE_SPEED / FIXED_HZ as f32);
    combat
        .solid_level(&level)
        .move_player(&mut player.position, displacement);
    player.tick += 1;
    combat::tick(&level, &player, actions.fire, interact, &mut combat);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(rows: &[&str], objects: &str) -> String {
        format!("(rows: {rows:?}, objects: [{objects}])")
    }

    const SPAWN: &str = "(id: \"spawn\", kind: Spawn, position: (2.5, 2.5), yaw: 0.0)";
    const ROOM: [&str; 5] = ["#####", "#...#", "#...#", "#...#", "#####"];

    fn room() -> Level {
        Level::parse(&source(&ROOM, SPAWN)).unwrap()
    }

    fn app(level: Level) -> App {
        let mut app = App::new();
        app.insert_resource(level).add_plugins(GameplayPlugin);
        app
    }

    fn act(app: &mut App, movement: Vec2, look_delta: Vec2, ticks: usize) {
        *app.world_mut().resource_mut::<GameplayActions>() = GameplayActions {
            movement,
            look_delta,
            ..default()
        };
        for _ in 0..ticks {
            app.world_mut().run_schedule(FixedUpdate);
        }
    }

    fn near(actual: Vec2, expected: Vec2) {
        assert!(
            actual.distance(expected) < 0.001,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn bundled_level_and_outside_are_solid() {
        let level = Level::demo();
        assert_eq!((level.width(), level.height()), (17, 13));
        assert_eq!(level.objects().len(), 10);
        assert_eq!(level.objects()[0].id, "player-spawn");
        assert!(!level.is_wall(8, 5));
        assert!(level.is_wall(8, 4));
        for (x, z) in [(-1, 2), (17, 2), (2, -1), (2, 13), (i32::MAX, i32::MIN)] {
            assert!(level.is_wall(x, z));
        }
        assert!(level.collides(Vec2::new(-1.0, 2.5)));
        assert!(level.collides(Vec2::new(f32::MAX, 2.5)));
        assert!(level.collides(Vec2::new(f32::NAN, 2.5)));
    }

    #[test]
    fn malformed_geometry_is_rejected() {
        let oversized = ".".repeat(MAX_DIMENSION + 1);
        for rows in [
            vec![],
            vec!["##", "##"],
            vec!["#####", "#...#", "####"],
            vec!["#####", "#x..#", "#####"],
            vec!["#####", "#é.#", "#####"],
            vec!["##.##", "#...#", "#####"],
            vec!["#####", "....#", "#####"],
            vec![oversized.as_str(); 3],
            vec!["#####"; MAX_DIMENSION + 1],
        ] {
            assert!(Level::parse(&source(&rows, SPAWN)).is_err(), "{rows:?}");
        }
        assert!(Level::parse("not RON").is_err());
        assert!(Level::parse("(rows: [], objects: [], mystery: 1)").is_err());
    }

    #[test]
    fn validation_errors_identify_the_authored_problem() {
        let rows = ["#####", "#x..#", "#####"];
        let error = Level::parse(&source(&rows, SPAWN)).unwrap_err().to_string();
        assert!(error.contains("unsupported cell at (1, 1)"), "{error}");
        let duplicate = format!("{SPAWN}, {}", SPAWN.replace("Spawn", "Marker"));
        let error = Level::parse(&source(&ROOM, &duplicate))
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate object ID: spawn"), "{error}");
        let invalid_id = SPAWN.replace("spawn", "bad id");
        let error = Level::parse(&source(&ROOM, &invalid_id))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("object 0 has invalid ID \"bad id\""),
            "{error}"
        );
        let wall_spawn = SPAWN.replace("(2.5, 2.5)", "(0.5, 2.5)");
        let error = Level::parse(&source(&ROOM, &wall_spawn))
            .unwrap_err()
            .to_string();
        assert!(error.contains("object spawn is not on floor"), "{error}");
        let error = Level::parse("(rows: [").unwrap_err().to_string();
        assert!(
            error.contains("invalid level RON:") && error.contains("1:"),
            "{error}"
        );
    }

    #[test]
    fn invalid_objects_and_spawn_clearance_are_rejected() {
        let marker = SPAWN.replace("Spawn", "Marker");
        let second_spawn = SPAWN.replace("spawn", "second");
        let duplicate_marker = marker.clone();
        for objects in [
            String::new(),
            marker,
            format!("{SPAWN}, {second_spawn}"),
            format!("{SPAWN}, {duplicate_marker}"),
            SPAWN.replace("spawn", ""),
            SPAWN.replace("spawn", "space id"),
            SPAWN.replace("spawn", &"a".repeat(65)),
            SPAWN.replace("(2.5, 2.5)", "(0.5, 2.5)"),
            SPAWN.replace("(2.5, 2.5)", "(-1.0, 2.5)"),
            SPAWN.replace("(2.5, 2.5)", "(5.0, 2.5)"),
            SPAWN.replace("(2.5, 2.5)", "(1.1, 2.5)"),
            SPAWN.replace("(2.5, 2.5)", "(NaN, 2.5)"),
            SPAWN.replace("(2.5, 2.5)", "(inf, 2.5)"),
            SPAWN.replace("yaw: 0.0", "yaw: inf"),
            SPAWN.replace("Spawn", "Unknown"),
            SPAWN.replace("yaw: 0.0", "yaw: 0.0, mystery: 1"),
            vec![SPAWN; MAX_OBJECTS + 1].join(","),
        ] {
            assert!(Level::parse(&source(&ROOM, &objects)).is_err(), "{objects}");
        }
    }

    #[test]
    fn radius_follows_round_corners_not_a_square() {
        let rows = ["######", "#....#", "#.#..#", "#....#", "#....#", "######"];
        // Near the wall's corner, both axis distances are below the radius,
        // but the Euclidean distance clears it. Closer diagonal poses do not.
        let clear = SPAWN.replace("(2.5, 2.5)", "(1.85, 1.85)");
        let level = Level::parse(&source(&rows, &clear)).unwrap();
        assert!(!level.collides(Vec2::new(1.85, 1.85)));
        assert!(level.collides(Vec2::new(1.87, 1.87)));
        assert!(Level::parse(&source(&rows, &clear.replace("1.85", "1.87"))).is_err());
        let mut position = Vec2::new(1.5, 1.5);
        for _ in 0..200 {
            level.move_player(&mut position, Vec2::splat(0.05));
            assert!(!level.collides(position));
        }
        // A closed room corner cannot be escaped even with diagonal input.
        let room = room();
        position = Vec2::splat(2.5);
        room.move_player(&mut position, Vec2::splat(20.0));
        near(position, Vec2::splat(4.0 - PLAYER_RADIUS));
        assert!(!room.collides(position));
    }

    #[test]
    fn movement_slides_and_cannot_tunnel_through_walls() {
        let level = room();
        let mut position = Vec2::new(1.21, 2.5);
        level.move_player(&mut position, Vec2::new(-0.5, -0.5));
        near(position, Vec2::new(1.0 + PLAYER_RADIUS, 2.0));
        assert!(!level.collides(position));
        level.move_player(&mut position, Vec2::new(-20.0, 0.0));
        near(position, Vec2::new(1.0 + PLAYER_RADIUS, 2.0));
        let rows = ["#######", "#..#..#", "#..#..#", "#..#..#", "#######"];
        let level = Level::parse(&source(&rows, SPAWN)).unwrap();
        position = Vec2::new(2.5, 2.5);
        level.move_player(&mut position, Vec2::new(20.0, 0.0));
        near(position, Vec2::new(3.0 - PLAYER_RADIUS, 2.5));
        assert!(!level.collides(position));
    }

    #[test]
    fn diagonal_speed_and_analog_movement_are_bounded() {
        for movement in [
            Vec2::Y,
            Vec2::ONE,
            Vec2::splat(100.0),
            Vec2::splat(f32::MAX),
        ] {
            let mut app = app(room());
            let start = app.world().resource::<PlayerState>().position;
            act(&mut app, movement, Vec2::ZERO, 10);
            let player = app.world().resource::<PlayerState>();
            assert!((player.position.distance(start) - 0.5).abs() < 0.001);
            assert_eq!(player.tick, 10);
        }
        let mut app = app(room());
        act(&mut app, Vec2::Y * 0.5, Vec2::ZERO, 10);
        near(
            app.world().resource::<PlayerState>().position,
            Vec2::new(2.5, 2.25),
        );
    }

    #[test]
    fn aim_is_consumed_once_and_turns_left_before_moving() {
        let mut app = app(room());
        act(&mut app, Vec2::Y, Vec2::new(PI / 2.0, 0.25), 2);
        let player = app.world().resource::<PlayerState>();
        near(player.position, Vec2::new(2.4, 2.5));
        assert!((player.yaw - PI / 2.0).abs() < 0.00001);
        assert_eq!(player.pitch, 0.25);
        assert_eq!(
            app.world().resource::<GameplayActions>().look_delta,
            Vec2::ZERO
        );
        act(&mut app, Vec2::X, Vec2::ZERO, 1);
        near(
            app.world().resource::<PlayerState>().position,
            Vec2::new(2.4, 2.45),
        );
        act(&mut app, Vec2::ZERO, Vec2::new(20.0 * PI, 100.0), 1);
        assert_eq!(app.world().resource::<PlayerState>().pitch, PITCH_LIMIT);
        act(&mut app, Vec2::ZERO, Vec2::new(f32::MAX, -f32::MAX), 1);
        let player = app.world().resource::<PlayerState>();
        assert!(player.yaw.is_finite() && (-PI..=PI).contains(&player.yaw));
        assert_eq!(player.pitch, -PITCH_LIMIT);
        act(
            &mut app,
            Vec2::splat(f32::NAN),
            Vec2::splat(f32::INFINITY),
            1,
        );
        let player = app.world().resource::<PlayerState>();
        assert!(player.position.is_finite() && player.yaw.is_finite() && player.pitch.is_finite());
    }

    fn corridor_route(extra_entities: usize) -> Vec<GameplayObservation> {
        let mut app = app(Level::parse(include_str!("../tests/foundation.ron")).unwrap());
        for _ in 0..extra_entities {
            app.world_mut().spawn_empty();
        }
        let mut observations = vec![GameplayObservation::capture(app.world())];
        // North through the first chamber, east through the corridor, north
        // into the second chamber. Interactive and headless callers use the
        // same held movement and once-per-tick aim resources.
        act(&mut app, Vec2::Y, Vec2::new(0.0, 0.1), 80);
        near(
            app.world().resource::<PlayerState>().position,
            Vec2::new(3.5, 5.5),
        );
        observations.push(GameplayObservation::capture(app.world()));
        act(&mut app, Vec2::ZERO, Vec2::new(-PI / 2.0, 0.0), 1);
        act(&mut app, Vec2::Y, Vec2::ZERO, 100);
        near(
            app.world().resource::<PlayerState>().position,
            Vec2::new(8.5, 5.5),
        );
        observations.push(GameplayObservation::capture(app.world()));
        act(&mut app, Vec2::Y, Vec2::ZERO, 100);
        near(
            app.world().resource::<PlayerState>().position,
            Vec2::new(13.5, 5.5),
        );
        observations.push(GameplayObservation::capture(app.world()));
        act(&mut app, Vec2::ZERO, Vec2::new(PI / 2.0, -0.1), 1);
        act(&mut app, Vec2::Y, Vec2::ZERO, 40);
        let final_state = GameplayObservation::capture(app.world());
        near(final_state.player.position, Vec2::new(13.5, 3.5));
        assert_eq!(final_state.player.tick, 322);
        assert_eq!(final_state.player.pitch, 0.0);
        assert!(final_state.player.yaw.abs() < 0.00001);
        assert_eq!(
            final_state.object_ids,
            ["industrial-marker", "player-spawn"]
        );
        observations.push(final_state);
        observations
    }

    #[test]
    fn repeated_shared_action_route_has_identical_observed_gameplay() {
        let expected = corridor_route(0);
        for extra_entities in [0, 1, 7, 29] {
            assert_eq!(corridor_route(extra_entities), expected);
        }
    }
}
