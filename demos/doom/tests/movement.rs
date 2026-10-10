//! Exercise the real fixed-tick app through Titan's controlled-time harness.

use bevy::prelude::*;
use core::f32::consts::FRAC_PI_2;
use titan_doom::{
    GameplayActions, GameplayObservation, GameplayPlugin, Level, PlayerState, FIXED_HZ,
    PLAYER_RADIUS,
};
use titan_test::Sim;

fn simulation() -> Sim {
    Sim::new(|app| {
        app.insert_resource(Level::parse(include_str!("foundation.ron")).unwrap())
            .add_plugins(GameplayPlugin);
    })
    .with_fixed_dt(1.0 / FIXED_HZ)
}

fn actions(sim: &mut Sim, movement: Vec2, look_delta: Vec2, ticks: u64) {
    *sim.world_mut().resource_mut::<GameplayActions>() = GameplayActions {
        movement,
        look_delta,
        ..default()
    };
    sim.run_ticks(ticks);
}

fn assert_position(sim: &Sim, position: Vec2) {
    let actual = sim.resource::<PlayerState>().position;
    assert!(
        actual.distance(position) < 0.001,
        "{actual:?} != {position:?}"
    );
}

fn scenario() -> Vec<GameplayObservation> {
    let mut sim = simulation();
    let mut observed = vec![GameplayObservation::capture(sim.world())];
    // The initial room, then both halves of the corridor, then the marker room.
    for (movement, aim, ticks, expected) in [
        (Vec2::Y, Vec2::ZERO, 80, Vec2::new(3.5, 5.5)),
        (
            Vec2::Y,
            Vec2::new(-FRAC_PI_2, 0.2),
            100,
            Vec2::new(8.5, 5.5),
        ),
        (Vec2::Y, Vec2::ZERO, 100, Vec2::new(13.5, 5.5)),
        (
            Vec2::Y,
            Vec2::new(FRAC_PI_2, -0.2),
            40,
            Vec2::new(13.5, 3.5),
        ),
    ] {
        actions(&mut sim, movement, aim, ticks);
        assert_position(&sim, expected);
        assert_eq!(sim.resource::<PlayerState>().tick, sim.current_tick());
        observed.push(GameplayObservation::capture(sim.world()));
    }
    assert_eq!(sim.current_tick(), 320);
    assert_eq!(
        observed.last().unwrap().object_ids,
        ["industrial-marker", "player-spawn"]
    );
    assert_eq!(sim.resource::<PlayerState>().pitch, 0.0);
    observed
}

#[test]
fn controlled_time_replays_room_corridor_movement() {
    let expected = scenario();
    for _ in 0..4 {
        assert_eq!(scenario(), expected);
    }
}

#[test]
fn pending_aim_survives_frames_without_a_fixed_tick() {
    let mut sim = simulation().with_fixed_dt(0.01);
    sim.world_mut()
        .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ));
    sim.world_mut().resource_mut::<GameplayActions>().look_delta = Vec2::new(0.1, 0.2);
    sim.tick();
    assert_eq!(sim.resource::<PlayerState>().tick, 0);
    assert_eq!(
        sim.resource::<GameplayActions>().look_delta,
        Vec2::new(0.1, 0.2)
    );
    sim.world_mut().resource_mut::<GameplayActions>().look_delta += Vec2::new(0.2, 0.1);
    sim.tick();
    let player = sim.resource::<PlayerState>();
    assert_eq!(player.tick, 1);
    assert!((player.yaw - 0.3).abs() < 0.00001);
    assert!((player.pitch - 0.3).abs() < 0.00001);
    assert_eq!(sim.resource::<GameplayActions>().look_delta, Vec2::ZERO);
}

#[test]
fn several_fixed_ticks_hold_movement_but_consume_aim_once() {
    let mut sim = simulation().with_fixed_dt(0.06);
    sim.world_mut()
        .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ));
    let start = sim.resource::<PlayerState>().position;
    actions(&mut sim, Vec2::Y, Vec2::new(0.2, 0.1), 1);
    let player = sim.resource::<PlayerState>();
    assert_eq!(player.tick, 3);
    assert!((player.yaw - 0.2).abs() < 0.00001);
    assert!((player.pitch - 0.1).abs() < 0.00001);
    assert!((player.position.distance(start) - 0.15).abs() < 0.001);
    assert_eq!(sim.resource::<GameplayActions>().look_delta, Vec2::ZERO);
    sim.tick();
    let player = sim.resource::<PlayerState>();
    assert_eq!(player.tick, 7);
    assert!((player.position.distance(start) - 0.35).abs() < 0.001);
    assert!((player.yaw - 0.2).abs() < 0.00001);
}

#[test]
fn shared_actions_hold_at_walls_and_a_room_corner() {
    let mut sim = simulation();
    // Turn north-west and hold against the enclosed corner for ten seconds.
    actions(
        &mut sim,
        Vec2::Y,
        Vec2::new(core::f32::consts::FRAC_PI_4, 0.0),
        600,
    );
    assert_position(&sim, Vec2::splat(1.0 + PLAYER_RADIUS));
    let contact = sim.resource::<PlayerState>().position;
    actions(&mut sim, Vec2::Y, Vec2::ZERO, 600);
    assert_eq!(sim.resource::<PlayerState>().position, contact);
    actions(&mut sim, Vec2::ZERO, Vec2::ZERO, 10);
    assert_eq!(sim.resource::<PlayerState>().position, contact);
}
