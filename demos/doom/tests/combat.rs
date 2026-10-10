//! Headless combat regressions driven by the same actions as human controls.

use bevy::{ecs::reflect::ReflectResource, prelude::*, reflect::ReflectRef};
use core::{
    any::TypeId,
    f32::consts::{FRAC_PI_2, PI},
};
use titan_doom::{
    CombatState, EnemyState, GamePhase, GameplayActions, GameplayEvent, GameplayObject,
    GameplayObservation, GameplayOutcome, GameplayPlugin, Level, ObjectKind, PlayerState,
    FIRE_COOLDOWN, MAX_AMMO, MAX_HEALTH, PLAYER_RADIUS, SHOT_DAMAGE,
};

const ROOM: [&str; 8] = [
    "########", "#......#", "#......#", "#......#", "#......#", "#......#", "#......#", "########",
];

fn placement(id: &str, kind: &str, x: f32, z: f32) -> String {
    format!("(id: {id:?}, kind: {kind}, position: ({x:?}, {z:?}), yaw: 0.0)")
}

fn fixture(rows: &[&str], objects: &[String]) -> App {
    app(Level::parse(&format!(
        "(rows: {rows:?}, objects: [{}])",
        objects.join(",")
    ))
    .unwrap())
}

fn app(level: Level) -> App {
    let mut app = App::new();
    app.insert_resource(level).add_plugins(GameplayPlugin);
    app
}

fn act(app: &mut App, actions: GameplayActions, ticks: usize) -> Vec<GameplayEvent> {
    *app.world_mut().resource_mut::<GameplayActions>() = actions;
    let mut events = Vec::new();
    for _ in 0..ticks {
        app.world_mut().run_schedule(FixedUpdate);
        events.extend(state(app).events.iter().cloned());
    }
    events
}

fn idle(app: &mut App, ticks: usize) -> Vec<GameplayEvent> {
    act(app, GameplayActions::default(), ticks)
}

fn fire(app: &mut App, ticks: usize) -> Vec<GameplayEvent> {
    act(
        app,
        GameplayActions {
            fire: true,
            ..default()
        },
        ticks,
    )
}

fn walk(app: &mut App, movement: Vec2, ticks: usize) -> Vec<GameplayEvent> {
    act(
        app,
        GameplayActions {
            movement,
            ..default()
        },
        ticks,
    )
}

fn state(app: &App) -> &CombatState {
    app.world().resource::<CombatState>()
}

fn object<'a>(app: &'a App, id: &str) -> &'a GameplayObject {
    state(app)
        .objects
        .iter()
        .find(|object| object.id == id)
        .unwrap()
}

fn outcome(events: &[GameplayEvent], expected: GameplayOutcome, id: Option<&str>) {
    assert!(
        events
            .iter()
            .any(|event| event.outcome == expected && event.object_id.as_deref() == id),
        "missing {expected:?} for {id:?}: {events:?}"
    );
}

fn near(actual: Vec2, expected: Vec2) {
    assert!(
        actual.distance(expected) < 0.001,
        "{actual:?} != {expected:?}"
    );
}

fn enemy_room() -> App {
    fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 6.5),
            placement("enemy", "Enemy", 3.5, 2.5),
        ],
    )
}

#[test]
fn held_fire_consumes_ammo_damages_and_respects_exact_cooldown() {
    let mut app = enemy_room();
    let ammo = state(&app).ammo;
    let health = object(&app, "enemy").health;
    let events = fire(&mut app, 1);
    assert_eq!(state(&app).ammo, ammo - 1);
    assert_eq!(object(&app, "enemy").health, health - SHOT_DAMAGE);
    assert_eq!(state(&app).weapon_cooldown, FIRE_COOLDOWN);
    assert_eq!(
        events,
        [GameplayEvent {
            tick: 1,
            object_id: Some("enemy".into()),
            outcome: GameplayOutcome::ShotHit
        }]
    );
    assert!(fire(&mut app, FIRE_COOLDOWN as usize - 1).is_empty());
    assert_eq!(state(&app).ammo, ammo - 1);
    assert_eq!(state(&app).weapon_cooldown, 1);
    let events = fire(&mut app, 1);
    outcome(&events, GameplayOutcome::ShotHit, Some("enemy"));
    outcome(&events, GameplayOutcome::EnemyKilled, Some("enemy"));
    assert_eq!(state(&app).ammo, ammo - 2);
    assert_eq!(object(&app, "enemy").health, 0);
    assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Dead);
    assert!(!object(&app, "enemy").active);
    let position = object(&app, "enemy").position;
    idle(&mut app, 180);
    assert_eq!(object(&app, "enemy").position, position);
    assert_eq!(state(&app).health, MAX_HEALTH);
}

#[test]
fn empty_weapon_never_underflows_or_damages_targets() {
    let mut app = enemy_room();
    // Look away while spending all twelve initial rounds through shared input.
    act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(PI, 0.0),
            fire: true,
            ..default()
        },
        1 + 11 * FIRE_COOLDOWN as usize,
    );
    assert_eq!(state(&app).ammo, 0);
    let health = object(&app, "enemy").health;
    let events = act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(-PI, 0.0),
            fire: true,
            ..default()
        },
        FIRE_COOLDOWN as usize,
    );
    outcome(&events, GameplayOutcome::EmptyAmmo, None);
    assert!(!events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::ShotHit));
    assert_eq!(object(&app, "enemy").health, health);
    assert_eq!(state(&app).ammo, 0);
}

#[test]
fn pitched_shot_misses_then_level_aim_hits() {
    let mut app = enemy_room();
    let events = act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(0.0, 1.0),
            fire: true,
            ..default()
        },
        1,
    );
    assert_eq!(state(&app).ammo, 11);
    assert_eq!(object(&app, "enemy").health, 50);
    assert!(!events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::ShotHit));
    idle(&mut app, FIRE_COOLDOWN as usize - 1);
    let events = act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(0.0, -1.0),
            fire: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::ShotHit, Some("enemy"));
    assert_eq!(object(&app, "enemy").health, 50 - SHOT_DAMAGE);
}

#[test]
fn shots_choose_nearest_target_not_authored_or_sorted_order() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 6.5),
            placement("a-far", "Enemy", 3.5, 1.5),
            placement("z-near", "Enemy", 3.5, 4.5),
        ],
    );
    let events = fire(&mut app, 1 + FIRE_COOLDOWN as usize);
    outcome(&events, GameplayOutcome::EnemyKilled, Some("z-near"));
    assert_eq!(object(&app, "a-far").health, 50);
    let events = fire(&mut app, FIRE_COOLDOWN as usize);
    outcome(&events, GameplayOutcome::ShotHit, Some("a-far"));
    assert_eq!(object(&app, "a-far").health, 50 - SHOT_DAMAGE);
}

#[test]
fn walls_and_closed_doors_block_shots_and_enemy_line_of_sight() {
    for door in [false, true] {
        let rows = [
            "#######",
            "#.....#",
            "#.....#",
            if door { "#.....#" } else { "#.###.#" },
            "#.....#",
            "#.....#",
            "#######",
        ];
        let mut objects = vec![
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("enemy", "Enemy", 3.5, 2.5),
        ];
        if door {
            objects.push(placement("door", "RedDoor", 3.5, 3.5));
        }
        let mut app = fixture(&rows, &objects);
        let initial = object(&app, "enemy").position;
        let events = fire(&mut app, 1);
        outcome(
            &events,
            GameplayOutcome::ShotBlocked,
            if door { Some("door") } else { None },
        );
        assert_eq!(state(&app).ammo, 11);
        idle(&mut app, 300);
        assert_eq!(object(&app, "enemy").health, 50);
        assert_eq!(object(&app, "enemy").position, initial);
        assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Idle);
        assert_eq!(state(&app).health, MAX_HEALTH);
    }
}

#[test]
fn even_melee_range_requires_unobstructed_sight_at_wall_and_door_corners() {
    for door in [false, true] {
        let rows = [
            "######",
            "#....#",
            "#....#",
            if door { "#....#" } else { "#..#.#" },
            "#....#",
            "######",
        ];
        let mut objects = vec![
            placement("spawn", "Spawn", 2.79, 3.3),
            placement("enemy", "Enemy", 3.3, 2.79),
        ];
        if door {
            objects.push(placement("door", "RedDoor", 3.5, 3.5));
        }
        let mut app = fixture(&rows, &objects);
        assert!(
            object(&app, "enemy")
                .position
                .distance(app.world().resource::<PlayerState>().position)
                < 0.8
        );
        idle(&mut app, 180);
        assert_eq!(state(&app).health, MAX_HEALTH);
        assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Idle);
        near(object(&app, "enemy").position, Vec2::new(3.3, 2.79));
    }
}

#[test]
fn closed_door_blocks_player_and_enemy_until_shared_interaction_opens_it() {
    let rows = [
        "#######", "###.###", "###.###", "###.###", "###.###", "###.###", "#######",
    ];
    let mut app = fixture(
        &rows,
        &[
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("enemy", "Enemy", 3.5, 1.5),
            placement("door", "RedDoor", 3.5, 3.5),
            placement("key", "RedKey", 3.5, 4.5),
        ],
    );
    walk(&mut app, Vec2::Y, 100);
    near(
        app.world().resource::<PlayerState>().position,
        Vec2::new(3.5, 4.0 + PLAYER_RADIUS),
    );
    assert_eq!(object(&app, "enemy").position, Vec2::new(3.5, 1.5));
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::DoorOpened, Some("door"));
    assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Chase);
    idle(&mut app, 70);
    assert!(
        object(&app, "enemy").position.y > 2.8,
        "enemy should traverse the now-open doorway"
    );
    walk(&mut app, Vec2::Y, 30);
    assert!(app.world().resource::<PlayerState>().position.y < 3.0);
}

#[test]
fn enemy_chases_attacks_on_cooldown_and_stops_when_killed() {
    let mut app = enemy_room();
    assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Idle);
    idle(&mut app, 1);
    assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Chase);
    near(object(&app, "enemy").position, Vec2::new(3.5, 2.52));
    let events = idle(&mut app, 180);
    outcome(&events, GameplayOutcome::PlayerDamaged, Some("enemy"));
    assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Attack);
    assert_eq!(state(&app).health, MAX_HEALTH - 12);
    let cooldown = object(&app, "enemy").cooldown;
    idle(&mut app, cooldown as usize - 1);
    assert_eq!(state(&app).health, MAX_HEALTH - 12);
    let events = idle(&mut app, 1);
    outcome(&events, GameplayOutcome::PlayerDamaged, Some("enemy"));
    assert_eq!(state(&app).health, MAX_HEALTH - 24);
    fire(&mut app, 1 + FIRE_COOLDOWN as usize);
    let health = state(&app).health;
    idle(&mut app, 180);
    assert_eq!(state(&app).health, health);
    assert_eq!(object(&app, "enemy").enemy_state, EnemyState::Dead);
}

#[test]
fn health_pickup_waits_at_full_health_then_heals_once_and_caps() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("enemy", "Enemy", 3.5, 4.0),
            placement("health", "Health", 3.5, 4.5),
        ],
    );
    let events = idle(&mut app, 1);
    assert!(!events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::HealthCollected));
    assert!(object(&app, "health").active);
    assert_eq!(state(&app).health, MAX_HEALTH - 12);
    let events = idle(&mut app, 1);
    outcome(&events, GameplayOutcome::HealthCollected, Some("health"));
    assert_eq!(state(&app).health, MAX_HEALTH);
    assert!(!object(&app, "health").active);
    let events = idle(&mut app, 60);
    assert_eq!(state(&app).health, MAX_HEALTH - 12);
    assert!(!events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::HealthCollected));
}

#[test]
fn ammo_pickups_collect_once_and_cap_without_consuming_at_capacity() {
    let mut objects = vec![placement("spawn", "Spawn", 3.5, 4.5)];
    for index in 0..10 {
        objects.push(placement(&format!("ammo-{index:02}"), "Ammo", 3.5, 4.5));
    }
    let mut app = fixture(&ROOM, &objects);
    let events = idle(&mut app, 1);
    assert_eq!(state(&app).ammo, MAX_AMMO);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.outcome == GameplayOutcome::AmmoCollected)
            .count(),
        8
    );
    assert!(object(&app, "ammo-08").active);
    assert!(object(&app, "ammo-09").active);
    assert!(idle(&mut app, 10).is_empty());
    let events = fire(&mut app, 1);
    outcome(&events, GameplayOutcome::AmmoCollected, Some("ammo-08"));
    assert_eq!(state(&app).ammo, MAX_AMMO);
    assert!(!object(&app, "ammo-08").active);
    // Leave the final unused pack and spend a round; consumed packs cannot refill it.
    walk(&mut app, Vec2::X, 20);
    let events = fire(&mut app, 1);
    assert_eq!(state(&app).ammo, MAX_AMMO - 1);
    assert!(!events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::AmmoCollected));
}

#[test]
fn door_reports_missing_key_then_opens_once_after_key_collection() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("door", "RedDoor", 3.5, 3.5),
            placement("key", "RedKey", 4.5, 4.5),
        ],
    );
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::MissingKey, Some("door"));
    assert!(object(&app, "door").active);
    assert!(!app.world().resource::<GameplayActions>().interact);
    assert!(idle(&mut app, 1).is_empty());
    let events = walk(&mut app, Vec2::X, 20);
    outcome(&events, GameplayOutcome::KeyCollected, Some("key"));
    assert!(state(&app).red_key);
    assert!(!object(&app, "key").active);
    walk(&mut app, -Vec2::X, 20);
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::DoorOpened, Some("door"));
    assert!(!object(&app, "door").active);
    assert!(state(&app).red_key);
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::NoDoor, None);
    walk(&mut app, Vec2::Y, 40);
    near(
        app.world().resource::<PlayerState>().position,
        Vec2::new(3.5, 2.5),
    );
}

#[test]
fn interaction_cannot_open_remote_or_behind_player_doors() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 6.5),
            placement("door", "RedDoor", 3.5, 3.5),
            placement("key", "RedKey", 3.5, 6.5),
        ],
    );
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::KeyCollected, Some("key"));
    outcome(&events, GameplayOutcome::NoDoor, None);
    assert!(object(&app, "door").active);
    walk(&mut app, Vec2::Y, 40);
    let events = act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(PI, 0.0),
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::NoDoor, None);
    assert!(object(&app, "door").active);
}

#[test]
fn nearby_pickups_cannot_be_collected_through_a_wall_corner() {
    let rows = ["######", "#....#", "#....#", "#..#.#", "#....#", "######"];
    let mut app = fixture(
        &rows,
        &[
            placement("spawn", "Spawn", 2.79, 3.3),
            placement("ammo", "Ammo", 3.05, 2.99),
            placement("key", "RedKey", 3.05, 2.99),
            placement("health", "Health", 3.05, 2.99),
        ],
    );
    assert!(
        object(&app, "key")
            .position
            .distance(app.world().resource::<PlayerState>().position)
            < 0.5
    );
    assert!(idle(&mut app, 60).is_empty());
    assert!(!state(&app).red_key);
    assert_eq!(state(&app).ammo, 12);
    for id in ["ammo", "key", "health"] {
        assert!(object(&app, id).active);
    }
}

#[test]
fn interaction_cannot_open_a_nearby_door_through_a_wall() {
    let rows = ["######", "#....#", "#....#", "#..#.#", "#....#", "######"];
    let mut app = fixture(
        &rows,
        &[
            placement("spawn", "Spawn", 2.79, 3.3),
            placement("key", "RedKey", 2.79, 3.3),
            placement("door", "RedDoor", 3.5, 2.5),
        ],
    );
    let delta = object(&app, "door").position - app.world().resource::<PlayerState>().position;
    assert!(delta.length() < 1.5);
    let events = act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(ops::atan2(-delta.x, -delta.y), 0.0),
            interact: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::KeyCollected, Some("key"));
    outcome(&events, GameplayOutcome::NoDoor, None);
    assert!(object(&app, "door").active);
}

#[test]
fn vertical_aim_reports_a_miss_without_hitting_or_ignoring_ammo_cost() {
    let mut app = enemy_room();
    let events = act(
        &mut app,
        GameplayActions {
            look_delta: Vec2::new(0.0, FRAC_PI_2),
            fire: true,
            ..default()
        },
        1,
    );
    outcome(&events, GameplayOutcome::ShotMiss, None);
    assert_eq!(object(&app, "enemy").health, 50);
    assert_eq!(state(&app).ammo, 11);
}

#[test]
fn key_and_door_can_be_collected_and_opened_in_the_same_tick() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("key", "RedKey", 3.5, 4.5),
            placement("door", "RedDoor", 3.5, 3.5),
        ],
    );
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    assert_eq!(
        events.iter().map(|event| event.outcome).collect::<Vec<_>>(),
        [GameplayOutcome::KeyCollected, GameplayOutcome::DoorOpened]
    );
    assert!(state(&app).red_key);
    assert!(!object(&app, "door").active);
}

fn assert_terminal_freeze(app: &mut App) {
    let mut expected = GameplayObservation::capture(app.world());
    expected.combat.events.clear();
    let events = act(
        app,
        GameplayActions {
            movement: Vec2::ONE,
            look_delta: Vec2::new(1.0, 0.5),
            fire: true,
            interact: true,
            ..default()
        },
        120,
    );
    assert!(events.is_empty());
    assert_eq!(GameplayObservation::capture(app.world()), expected);
    assert_eq!(
        app.world().resource::<GameplayActions>().look_delta,
        Vec2::ZERO
    );
    assert!(!app.world().resource::<GameplayActions>().interact);
}

fn assert_clean_restart(app: &mut App, initial: &GameplayObservation) {
    let events = act(
        app,
        GameplayActions {
            movement: Vec2::ONE,
            look_delta: Vec2::ONE,
            fire: true,
            interact: true,
            restart: true,
        },
        1,
    );
    assert_eq!(
        events,
        [GameplayEvent {
            tick: 0,
            object_id: None,
            outcome: GameplayOutcome::Restarted
        }]
    );
    let mut actual = GameplayObservation::capture(app.world());
    actual.combat.events.clear();
    assert_eq!(&actual, initial);
    let actions = app.world().resource::<GameplayActions>();
    assert_eq!(actions.movement, Vec2::ZERO);
    assert_eq!(actions.look_delta, Vec2::ZERO);
    assert!(!actions.fire && !actions.interact && !actions.restart);
}

#[test]
fn death_freezes_simulation_and_restart_restores_clean_authored_state() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("enemy", "Enemy", 3.5, 4.25),
            placement("key", "RedKey", 3.5, 4.5),
            placement("ammo", "Ammo", 3.5, 4.5),
            placement("door", "RedDoor", 3.5, 3.5),
        ],
    );
    let initial = GameplayObservation::capture(app.world());
    act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    assert!(
        !object(&app, "key").active && !object(&app, "ammo").active && !object(&app, "door").active
    );
    let events = idle(&mut app, 480);
    outcome(&events, GameplayOutcome::PlayerDied, None);
    assert_eq!(state(&app).phase, GamePhase::Dead);
    assert_eq!(state(&app).health, 0);
    assert_terminal_freeze(&mut app);
    assert_clean_restart(&mut app, &initial);
    let events = idle(&mut app, 1);
    outcome(&events, GameplayOutcome::PlayerDamaged, Some("enemy"));
    assert_eq!(app.world().resource::<PlayerState>().tick, 1);
}

#[test]
fn restart_during_combat_clears_weapon_cooldown_pose_and_held_inputs() {
    let mut app = enemy_room();
    let initial = GameplayObservation::capture(app.world());
    act(
        &mut app,
        GameplayActions {
            movement: Vec2::X,
            look_delta: Vec2::new(0.1, 0.2),
            fire: true,
            ..default()
        },
        1,
    );
    assert_eq!(state(&app).ammo, 11);
    assert_eq!(state(&app).weapon_cooldown, FIRE_COOLDOWN);
    assert_ne!(app.world().resource::<PlayerState>(), &initial.player);
    assert_clean_restart(&mut app, &initial);
    let events = idle(&mut app, 1);
    assert!(events.is_empty());
    assert_eq!(state(&app).ammo, initial.combat.ammo);
    assert_eq!(state(&app).weapon_cooldown, 0);
    near(
        app.world().resource::<PlayerState>().position,
        initial.player.position,
    );
}

#[test]
fn exit_wins_freezes_simulation_and_supports_clean_restart() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("spawn", "Spawn", 3.5, 4.5),
            placement("exit", "Exit", 3.5, 2.5),
            placement("enemy", "Enemy", 1.5, 1.5),
        ],
    );
    let initial = GameplayObservation::capture(app.world());
    let events = walk(&mut app, Vec2::Y, 40);
    outcome(&events, GameplayOutcome::ExitReached, Some("exit"));
    assert_eq!(state(&app).phase, GamePhase::Won);
    assert!(!object(&app, "exit").active);
    assert_terminal_freeze(&mut app);
    assert_clean_restart(&mut app, &initial);
}

#[test]
fn combat_resources_and_structured_outcomes_are_registered_for_reflection() {
    let app = enemy_room();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    for id in [
        TypeId::of::<PlayerState>(),
        TypeId::of::<GameplayActions>(),
        TypeId::of::<CombatState>(),
    ] {
        let registration = registry.get(id).expect("gameplay resource registered");
        assert!(
            registration.data::<ReflectResource>().is_some(),
            "resource reflection enabled"
        );
    }
    for id in [
        TypeId::of::<GameplayObject>(),
        TypeId::of::<ObjectKind>(),
        TypeId::of::<EnemyState>(),
        TypeId::of::<GamePhase>(),
        TypeId::of::<GameplayEvent>(),
        TypeId::of::<GameplayOutcome>(),
    ] {
        assert!(registry.get(id).is_some());
    }
    let ReflectRef::Struct(reflected) = state(&app).reflect_ref() else {
        panic!("CombatState is reflected as a struct")
    };
    for field in [
        "health",
        "ammo",
        "red_key",
        "phase",
        "weapon_cooldown",
        "objects",
        "events",
    ] {
        assert!(
            reflected.field(field).is_some(),
            "missing observable field {field}"
        );
    }
}

#[test]
fn events_are_tick_scoped_and_object_states_have_sorted_stable_ids() {
    let mut app = fixture(
        &ROOM,
        &[
            placement("z-spawn", "Spawn", 3.5, 4.5),
            placement("a-enemy", "Enemy", 3.5, 2.5),
            placement("m-marker", "Marker", 1.5, 1.5),
        ],
    );
    let observation = GameplayObservation::capture(app.world());
    assert_eq!(observation.object_ids, ["a-enemy", "m-marker", "z-spawn"]);
    assert_eq!(
        observation
            .combat
            .objects
            .iter()
            .map(|object| object.id.as_str())
            .collect::<Vec<_>>(),
        ["a-enemy", "m-marker", "z-spawn"]
    );
    let events = fire(&mut app, 1);
    assert!(events.iter().all(|event| event.tick == 1));
    outcome(&events, GameplayOutcome::ShotHit, Some("a-enemy"));
    idle(&mut app, 1);
    assert!(state(&app).events.is_empty());
}

fn kill_remaining_enemy(app: &mut App) -> Vec<GameplayEvent> {
    let mut events = Vec::new();
    for _ in 0..2 {
        let target = state(app)
            .objects
            .iter()
            .find(|object| object.kind == ObjectKind::Enemy && object.active)
            .expect("remaining demo guard")
            .position;
        let player = app.world().resource::<PlayerState>();
        let delta = target - player.position;
        let yaw = ops::atan2(-delta.x, -delta.y);
        let look_delta = Vec2::new(yaw - player.yaw, -player.pitch);
        events.extend(act(
            app,
            GameplayActions {
                look_delta,
                fire: true,
                ..default()
            },
            1,
        ));
        events.extend(idle(app, FIRE_COOLDOWN as usize - 1));
    }
    events
}

fn face(app: &mut App, yaw: f32) -> Vec<GameplayEvent> {
    let player = app.world().resource::<PlayerState>();
    let look_delta = Vec2::new(yaw - player.yaw, -player.pitch);
    act(
        app,
        GameplayActions {
            look_delta,
            ..default()
        },
        1,
    )
}

fn demo_completion() -> (Vec<GameplayObservation>, Vec<GameplayEvent>) {
    let mut app = app(Level::demo());
    let initial = GameplayObservation::capture(app.world());
    let mut observations = vec![initial.clone()];
    let mut events = Vec::new();
    // Corridor enemy first; no teleports or direct combat-state mutation.
    events.extend(fire(&mut app, 1 + FIRE_COOLDOWN as usize));
    assert!(state(&app)
        .objects
        .iter()
        .filter(|object| object.kind == ObjectKind::Enemy)
        .any(|object| !object.active));
    observations.push(GameplayObservation::capture(app.world()));
    // Walk over the starting ammo, back to corridor center, then east to the guard room.
    events.extend(walk(&mut app, Vec2::X, 20));
    events.extend(walk(&mut app, -Vec2::X, 20));
    events.extend(walk(&mut app, Vec2::Y, 80));
    events.extend(act(
        &mut app,
        GameplayActions {
            movement: Vec2::Y,
            look_delta: Vec2::new(-FRAC_PI_2, 0.0),
            ..default()
        },
        200,
    ));
    near(
        app.world().resource::<PlayerState>().position,
        Vec2::new(13.5, 5.5),
    );
    // Aim at the observed guard, which may have chased diagonally into the corridor.
    events.extend(kill_remaining_enemy(&mut app));
    events.extend(face(&mut app, PI));
    assert!(state(&app)
        .objects
        .iter()
        .filter(|object| object.kind == ObjectKind::Enemy)
        .all(|object| !object.active));
    observations.push(GameplayObservation::capture(app.world()));
    events.extend(walk(&mut app, Vec2::Y, 80));
    near(
        app.world().resource::<PlayerState>().position,
        Vec2::new(13.5, 9.5),
    );
    assert!(state(&app).red_key);
    observations.push(GameplayObservation::capture(app.world()));
    // North to the locked exit alcove; its row-three barrier has only the red doorway.
    events.extend(act(
        &mut app,
        GameplayActions {
            movement: Vec2::Y,
            look_delta: Vec2::new(PI, 0.0),
            ..default()
        },
        100,
    ));
    near(
        app.world().resource::<PlayerState>().position,
        Vec2::new(13.5, 4.5),
    );
    events.extend(act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    ));
    observations.push(GameplayObservation::capture(app.world()));
    events.extend(walk(&mut app, Vec2::Y, 60));
    assert_eq!(state(&app).phase, GamePhase::Won);
    assert!(state(&app).health > 0);
    assert_eq!(
        state(&app)
            .objects
            .iter()
            .filter(|object| object.kind == ObjectKind::RedDoor && object.active)
            .count(),
        0
    );
    assert!(events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::AmmoCollected));
    for expected in [
        GameplayOutcome::EnemyKilled,
        GameplayOutcome::KeyCollected,
        GameplayOutcome::DoorOpened,
        GameplayOutcome::ExitReached,
    ] {
        assert!(
            events.iter().any(|event| event.outcome == expected),
            "missing demo milestone {expected:?}"
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event.outcome == GameplayOutcome::EnemyKilled)
            .count(),
        2
    );
    observations.push(GameplayObservation::capture(app.world()));
    assert_terminal_freeze(&mut app);
    assert_clean_restart(&mut app, &initial);
    (observations, events)
}

#[test]
fn full_authored_demo_completes_and_repeats_via_shared_actions() {
    let expected = demo_completion();
    for _ in 0..3 {
        assert_eq!(demo_completion(), expected);
    }
}

#[test]
fn authored_exit_room_cannot_bypass_the_locked_door() {
    let mut app = app(Level::demo());
    let level = app.world().resource::<Level>();
    for x in 10..=15 {
        assert_eq!(level.is_wall(x, 3), x != 13, "exit-room barrier at x={x}");
    }
    // Kill both enemies, then approach the doorway without visiting the key.
    fire(&mut app, 1 + FIRE_COOLDOWN as usize);
    walk(&mut app, Vec2::Y, 80);
    act(
        &mut app,
        GameplayActions {
            movement: Vec2::Y,
            look_delta: Vec2::new(-FRAC_PI_2, 0.0),
            ..default()
        },
        200,
    );
    kill_remaining_enemy(&mut app);
    face(&mut app, 0.0);
    walk(&mut app, Vec2::Y, 100);
    assert!(!state(&app).red_key);
    assert_eq!(state(&app).phase, GamePhase::Playing);
    near(
        app.world().resource::<PlayerState>().position,
        Vec2::new(13.5, 4.0 + PLAYER_RADIUS),
    );
    let events = act(
        &mut app,
        GameplayActions {
            interact: true,
            ..default()
        },
        1,
    );
    assert!(events
        .iter()
        .any(|event| event.outcome == GameplayOutcome::MissingKey));
}
