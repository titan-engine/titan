//! Local, deterministic combat and interaction rules for the demo.

use bevy::prelude::*;

#[cfg(test)]
#[path = "combat/ray_tests.rs"]
mod ray_tests;

use crate::{GameplayActions, Level, LevelError, ObjectKind, PlayerState, FIXED_HZ};

/// Number of ticks between weapon shots (four shots per second).
pub const FIRE_COOLDOWN: u32 = 15;
/// Damage inflicted by one successful hitscan shot.
pub const SHOT_DAMAGE: u32 = 25;
/// Maximum player health.
pub const MAX_HEALTH: u32 = 100;
/// Maximum carried ammunition.
pub const MAX_AMMO: u32 = 99;
const ENEMY_RADIUS: f32 = 0.35;
const SHOT_RANGE: f32 = 32.0;

/// Terminal gameplay status; only restart changes a terminal status.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePhase {
    /// Normal simulation.
    Playing,
    /// Player health reached zero.
    Dead,
    /// Player reached an exit.
    Won,
}

/// State of the single supported opponent type.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnemyState {
    /// No clear view of the player.
    Idle,
    /// Moving directly toward a visible player.
    Chase,
    /// In melee range with an unobstructed view.
    Attack,
    /// No longer moves or attacks.
    Dead,
}

/// Mutable gameplay state for one authored object, keyed by stable ID.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct GameplayObject {
    /// Persistent authored identity.
    pub id: String,
    /// Role from the validated level.
    pub kind: ObjectKind,
    /// Current `(x, z)` position; enemies may move.
    pub position: Vec2,
    /// Enemy hit points (zero for other objects).
    pub health: u32,
    /// Opponent behavior, ignored by other object kinds.
    pub enemy_state: EnemyState,
    /// Remaining enemy attack cooldown in ticks.
    pub cooldown: u32,
    /// False after collection, door opening, or enemy death.
    pub active: bool,
}

/// Structured explanations of important gameplay interactions.
#[derive(Reflect, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameplayOutcome {
    /// The shot damaged a target.
    ShotHit,
    /// Solid geometry or a closed door stopped the shot.
    ShotBlocked,
    /// No target intersected the shot.
    ShotMiss,
    /// The trigger was pulled without ammunition.
    EmptyAmmo,
    /// An opponent was killed.
    EnemyKilled,
    /// An opponent damaged the player.
    PlayerDamaged,
    /// The player died.
    PlayerDied,
    /// A health pack was consumed.
    HealthCollected,
    /// An ammo pack was consumed.
    AmmoCollected,
    /// The red key was collected.
    KeyCollected,
    /// The nearest door requires the red key.
    MissingKey,
    /// The nearest door opened.
    DoorOpened,
    /// No closed door is within interaction reach and view.
    NoDoor,
    /// The player reached an exit.
    ExitReached,
    /// All gameplay state was reset to the authored initial state.
    Restarted,
}

/// One outcome associated with a simulation tick and optional stable object ID.
#[derive(Reflect, Clone, Debug, PartialEq, Eq)]
pub struct GameplayEvent {
    /// Completed gameplay tick (zero for restart).
    pub tick: u64,
    /// Authored target, pickup, door, attacker, or exit identity, when applicable.
    pub object_id: Option<String>,
    /// Machine-readable interaction result.
    pub outcome: GameplayOutcome,
}

/// Reflected gameplay state independent of presentation and runtime entities.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource)]
pub struct CombatState {
    /// Current player hit points.
    pub health: u32,
    /// Remaining weapon rounds.
    pub ammo: u32,
    /// Whether the reusable red key has been collected.
    pub red_key: bool,
    /// Playing, dead, or won.
    pub phase: GamePhase,
    /// Remaining weapon cooldown in ticks.
    pub weapon_cooldown: u32,
    /// Objects sorted by stable authored ID for deterministic processing.
    pub objects: Vec<GameplayObject>,
    /// Outcomes from the latest tick only; bounded and replaced every tick.
    pub events: Vec<GameplayEvent>,
}

impl CombatState {
    pub(crate) fn new(level: &Level) -> Self {
        let mut objects: Vec<_> = level
            .objects()
            .iter()
            .map(|object| GameplayObject {
                id: object.id.clone(),
                kind: object.kind,
                position: object.position,
                health: if object.kind == ObjectKind::Enemy {
                    50
                } else {
                    0
                },
                enemy_state: EnemyState::Idle,
                cooldown: 0,
                active: true,
            })
            .collect();
        objects.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            health: MAX_HEALTH,
            ammo: 12,
            red_key: false,
            phase: GamePhase::Playing,
            weapon_cooldown: 0,
            objects,
            events: Vec::new(),
        }
    }

    pub(crate) fn solid_level(&self, level: &Level) -> Level {
        let mut solid = level.clone();
        for object in &self.objects {
            if object.kind == ObjectKind::RedDoor && object.active {
                set_cell_wall(&mut solid, object.position, true);
            }
        }
        solid
    }

    fn record(&mut self, tick: u64, id: Option<String>, outcome: GameplayOutcome) {
        self.events.push(GameplayEvent {
            tick,
            object_id: id,
            outcome,
        });
    }
}

pub(crate) fn validate_objects(level: &Level) -> Result<(), LevelError> {
    let mut doors = std::collections::HashSet::new();
    for object in level.objects() {
        if object.kind == ObjectKind::RedDoor {
            if object.position.fract() != Vec2::splat(0.5) {
                return Err(LevelError(format!(
                    "door {} must be at a cell center",
                    object.id
                )));
            }
            if !doors.insert(object.position.floor().as_ivec2()) {
                return Err(LevelError(format!("duplicate door cell for {}", object.id)));
            }
            for other in level.objects() {
                if other.id != object.id && other.position.floor() == object.position.floor() {
                    return Err(LevelError(format!(
                        "object {} overlaps door {}",
                        other.id, object.id
                    )));
                }
            }
        }
        if object.kind == ObjectKind::Enemy && level.collides(object.position) {
            return Err(LevelError(format!(
                "enemy {} lacks radius clearance",
                object.id
            )));
        }
    }
    let solid = CombatState::new(level).solid_level(level);
    for object in level.objects() {
        if object.kind == ObjectKind::Enemy && solid.collides(object.position) {
            return Err(LevelError(format!(
                "enemy {} lacks closed-door clearance",
                object.id
            )));
        }
    }
    let spawn = level
        .objects()
        .iter()
        .find(|o| o.kind == ObjectKind::Spawn)
        .unwrap();
    if solid.collides(spawn.position) {
        return Err(LevelError("spawn lacks closed-door clearance".into()));
    }
    Ok(())
}

pub(crate) fn restart(
    level: &Level,
    player: &mut PlayerState,
    actions: &mut GameplayActions,
    state: &mut CombatState,
) {
    let spawn = level
        .objects()
        .iter()
        .find(|o| o.kind == ObjectKind::Spawn)
        .unwrap();
    *player = PlayerState {
        position: spawn.position,
        yaw: crate::wrap_yaw(spawn.yaw),
        pitch: 0.0,
        tick: 0,
    };
    *actions = GameplayActions::default();
    *state = CombatState::new(level);
    state.record(0, None, GameplayOutcome::Restarted);
}

// A replaced level can be smaller than the current gameplay state's level.
// Never alias cells or index out of bounds while awaiting an explicit restart.
fn set_cell_wall(level: &mut Level, position: Vec2, wall: bool) -> bool {
    if !position.is_finite()
        || position.x < 0.0
        || position.y < 0.0
        || position.x >= level.width as f32
        || position.y >= level.height as f32
    {
        return false;
    }
    let cell = position.floor().as_uvec2();
    level.walls[cell.y as usize * level.width + cell.x as usize] = wall;
    true
}

// Supercover grid traversal: inspect only cells touched by the ray, including
// both sides of boundary-aligned rays and the side cells at corner crossings.
fn wall_distance(level: &Level, origin: Vec2, direction: Vec2, range: f32) -> f32 {
    grid_ray_distance(origin, direction, range, |cell| {
        level.is_wall(cell.x, cell.y)
    })
}

fn grid_ray_distance(
    origin: Vec2,
    direction: Vec2,
    range: f32,
    mut is_wall: impl FnMut(IVec2) -> bool,
) -> f32 {
    let mut cell = origin.floor().as_ivec2();
    let origin_edge = origin.cmpeq(origin.floor());
    let mut blocked = |cell: IVec2, x_edge: bool, z_edge: bool| {
        is_wall(cell)
            || (x_edge && is_wall(cell - IVec2::X))
            || (z_edge && is_wall(cell - IVec2::Y))
            || (x_edge && z_edge && is_wall(cell - IVec2::ONE))
    };
    // A wall touched at the starting point blocks even when facing away.
    if blocked(cell, origin_edge.x, origin_edge.y) {
        return 0.0;
    }
    let step = IVec2::new(
        i32::from(direction.x > 0.0) - i32::from(direction.x < 0.0),
        i32::from(direction.y > 0.0) - i32::from(direction.y < 0.0),
    );
    let parallel_x_edge = direction.x == 0.0 && origin_edge.x;
    let parallel_z_edge = direction.y == 0.0 && origin_edge.y;
    loop {
        // Recompute from integer boundaries rather than accumulate t deltas,
        // avoiding drift that could skip a later exact corner contact.
        let next = Vec2::new(
            if direction.x == 0.0 {
                f32::INFINITY
            } else {
                ((cell.x + i32::from(step.x > 0)) as f32 - origin.x) / direction.x
            },
            if direction.y == 0.0 {
                f32::INFINITY
            } else {
                ((cell.y + i32::from(step.y > 0)) as f32 - origin.y) / direction.y
            },
        );
        let distance = next.min_element();
        if distance > range {
            return range;
        }
        let cross_x = next.x <= next.y;
        let cross_z = next.y <= next.x;
        // At a tie, test both side cells before entering the diagonal cell.
        // A ray cannot slip between walls merely by touching their corner.
        if cross_x
            && blocked(
                cell + IVec2::new(step.x, 0),
                parallel_x_edge,
                parallel_z_edge,
            )
        {
            return distance;
        }
        if cross_z
            && blocked(
                cell + IVec2::new(0, step.y),
                parallel_x_edge,
                parallel_z_edge,
            )
        {
            return distance;
        }
        if cross_x {
            cell.x += step.x;
        }
        if cross_z {
            cell.y += step.y;
        }
        if cross_x && cross_z && blocked(cell, parallel_x_edge, parallel_z_edge) {
            return distance;
        }
    }
}

fn visible(level: &Level, from: Vec2, to: Vec2) -> bool {
    let distance = from.distance(to);
    distance < 0.00001 || wall_distance(level, from, (to - from) / distance, distance) >= distance
}

pub(crate) fn tick(
    level: &Level,
    player: &PlayerState,
    fire: bool,
    interact: bool,
    state: &mut CombatState,
) {
    let tick = player.tick;
    state.weapon_cooldown = state.weapon_cooldown.saturating_sub(1);
    let solid = state.solid_level(level);
    let (sin, cos) = ops::sin_cos(player.yaw);
    let forward = Vec2::new(-sin, -cos);
    if fire && state.weapon_cooldown == 0 {
        state.weapon_cooldown = FIRE_COOLDOWN;
        if state.ammo == 0 {
            state.record(tick, None, GameplayOutcome::EmptyAmmo);
        } else {
            state.ammo -= 1;
            let (pitch_sin, pitch_cos) = ops::sin_cos(player.pitch);
            let direction = Vec3::new(forward.x * pitch_cos, pitch_sin, forward.y * pitch_cos);
            let blocked = wall_distance(&solid, player.position, forward * pitch_cos, SHOT_RANGE);
            let mut nearest = blocked;
            let mut target = None;
            for (index, enemy) in state.objects.iter().enumerate() {
                if enemy.kind != ObjectKind::Enemy || !enemy.active {
                    continue;
                }
                let offset = Vec3::new(
                    enemy.position.x - player.position.x,
                    0.0,
                    enemy.position.y - player.position.y,
                );
                let along = offset.dot(direction);
                let discriminant =
                    ENEMY_RADIUS * ENEMY_RADIUS - (offset.length_squared() - along * along);
                if discriminant >= 0.0 {
                    let distance = (along - ops::sqrt(discriminant)).max(0.0);
                    if along + ops::sqrt(discriminant) >= 0.0 && distance < nearest {
                        nearest = distance;
                        target = Some(index);
                    }
                }
            }
            if let Some(index) = target {
                let enemy = &mut state.objects[index];
                enemy.health = enemy.health.saturating_sub(SHOT_DAMAGE);
                let id = enemy.id.clone();
                let dead = enemy.health == 0;
                if dead {
                    enemy.active = false;
                    enemy.enemy_state = EnemyState::Dead;
                }
                state.record(tick, Some(id.clone()), GameplayOutcome::ShotHit);
                if dead {
                    state.record(tick, Some(id), GameplayOutcome::EnemyKilled);
                }
            } else {
                let door = state
                    .objects
                    .iter()
                    .find(|o| {
                        o.kind == ObjectKind::RedDoor
                            && o.active
                            && (player.position + forward * pitch_cos * (blocked + 0.001)).floor()
                                == o.position.floor()
                    })
                    .map(|o| o.id.clone());
                state.record(
                    tick,
                    door,
                    if blocked < SHOT_RANGE {
                        GameplayOutcome::ShotBlocked
                    } else {
                        GameplayOutcome::ShotMiss
                    },
                );
            }
        }
    }
    // Pickups before interaction allow collecting a key and using it on the same tick.
    for index in 0..state.objects.len() {
        let object = &state.objects[index];
        if !object.active
            || player.position.distance(object.position) > 0.5
            || !visible(&solid, player.position, object.position)
        {
            continue;
        }
        let outcome = match object.kind {
            ObjectKind::Health if state.health < MAX_HEALTH => {
                state.health = (state.health + 30).min(MAX_HEALTH);
                GameplayOutcome::HealthCollected
            }
            ObjectKind::Ammo if state.ammo < MAX_AMMO => {
                state.ammo = (state.ammo + 12).min(MAX_AMMO);
                GameplayOutcome::AmmoCollected
            }
            ObjectKind::RedKey => {
                state.red_key = true;
                GameplayOutcome::KeyCollected
            }
            ObjectKind::Exit => {
                state.phase = GamePhase::Won;
                GameplayOutcome::ExitReached
            }
            _ => continue,
        };
        let object = &mut state.objects[index];
        object.active = false;
        let id = object.id.clone();
        state.record(tick, Some(id), outcome);
    }
    if interact {
        // Ignore only the target door's own cell when checking reachability;
        // walls and other closed doors still prevent interaction through them.
        let door = state
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.kind == ObjectKind::RedDoor && o.active)
            .filter(|(_, o)| {
                let delta = o.position - player.position;
                let distance = delta.length();
                distance <= 1.5 && delta.normalize_or_zero().dot(forward) >= 0.5 && {
                    let mut view = solid.clone();
                    set_cell_wall(&mut view, o.position, false)
                        && visible(&view, player.position, o.position)
                }
            })
            .min_by(|(_, a), (_, b)| {
                a.position
                    .distance_squared(player.position)
                    .total_cmp(&b.position.distance_squared(player.position))
            })
            .map(|(index, _)| index);
        if let Some(index) = door {
            let id = state.objects[index].id.clone();
            if state.red_key {
                state.objects[index].active = false;
                state.record(tick, Some(id), GameplayOutcome::DoorOpened);
            } else {
                state.record(tick, Some(id), GameplayOutcome::MissingKey);
            }
        } else {
            state.record(tick, None, GameplayOutcome::NoDoor);
        }
    }
    if state.phase != GamePhase::Playing {
        return;
    }
    let solid = state.solid_level(level);
    for index in 0..state.objects.len() {
        let enemy = &mut state.objects[index];
        if enemy.kind != ObjectKind::Enemy || !enemy.active {
            continue;
        }
        enemy.cooldown = enemy.cooldown.saturating_sub(1);
        let distance = enemy.position.distance(player.position);
        if distance > 10.0 || !visible(&solid, enemy.position, player.position) {
            enemy.enemy_state = EnemyState::Idle;
        } else if distance <= 0.8 {
            enemy.enemy_state = EnemyState::Attack;
            if enemy.cooldown == 0 {
                enemy.cooldown = 60;
                let id = enemy.id.clone();
                state.health = state.health.saturating_sub(12);
                state.record(tick, Some(id), GameplayOutcome::PlayerDamaged);
                if state.health == 0 {
                    state.phase = GamePhase::Dead;
                    state.record(tick, None, GameplayOutcome::PlayerDied);
                    break;
                }
            }
        } else {
            enemy.enemy_state = EnemyState::Chase;
            let displacement =
                (player.position - enemy.position).normalize_or_zero() * (1.2 / FIXED_HZ as f32);
            // Same circular wall/door collision as the player; never tunnel.
            solid.move_player(&mut enemy.position, displacement);
        }
    }
}
