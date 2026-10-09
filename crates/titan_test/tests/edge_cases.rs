//! Boundary cases for timing, script ordering, and diagnostics.

use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, ButtonInput};
use bevy_time::{Fixed, Time};
use core::time::Duration;
use std::panic::{catch_unwind, AssertUnwindSafe};
use titan_test::{InputScript, Sim};

#[derive(Resource, Default)]
struct FixedCount(u64);

#[test]
fn fixed_loop_accumulates_fractional_ticks_and_runs_multiple_steps() {
    for (fixed_ms, expected) in [(40, 4), (10, 16)] {
        let mut sim = Sim::new(|app| {
            app.init_resource::<FixedCount>()
                .add_systems(FixedUpdate, |mut count: ResMut<FixedCount>| count.0 += 1);
        })
        .with_fixed_dt(0.02);
        sim.world_mut()
            .resource_mut::<Time<Fixed>>()
            .set_timestep(Duration::from_millis(fixed_ms));
        sim.run_ticks(8);
        assert_eq!(sim.resource::<FixedCount>().0, expected);
        assert_eq!(sim.current_tick(), 8);
    }
}

#[test]
fn queued_input_is_not_replayed_while_waiting_for_a_fixed_step() {
    let mut sim = Sim::new(|_| {}).with_fixed_dt(0.01);
    sim.world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(Duration::from_secs(1));
    sim.tap(KeyCode::Space);
    assert!(sim
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::Space));
    sim.tick();
    assert!(sim
        .resource::<ButtonInput<KeyCode>>()
        .just_released(KeyCode::Space));
    sim.tick();
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .just_released(KeyCode::Space));
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Space));
}

#[test]
fn unsorted_script_retains_equal_tick_order_and_tap_release_precedes_next_press() {
    let script = InputScript::from_ron(
        "(version:1,events:[
            (tick:1,action:Press(Key(Space))),
            (tick:0,action:Tap(Key(Space))),
            (tick:0,action:Press(Key(KeyA))),
            (tick:0,action:Release(Key(KeyA))),
        ])",
    )
    .unwrap();
    let mut sim = Sim::new(|_| {});
    sim.run_script(&script, 1);
    assert!(sim
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Space));
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::KeyA));
    sim.run_script(&script, 2);
    let keys = sim.resource::<ButtonInput<KeyCode>>();
    assert!(keys.pressed(KeyCode::Space));
    assert!(keys.just_pressed(KeyCode::Space));
    sim.tick();
    assert!(sim
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Space));
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::Space));
}

#[test]
fn endpoint_tap_releases_when_resuming_with_an_ordinary_tick() {
    let script =
        InputScript::from_ron("(version:1,events:[(tick:0,action:Tap(Key(Space)))])").unwrap();
    let mut sim = Sim::new(|_| {});
    sim.run_script(&script, 1);
    sim.tick();
    assert!(sim
        .resource::<ButtonInput<KeyCode>>()
        .just_released(KeyCode::Space));
}

#[test]
fn unsupported_script_version_fails_before_advancing_or_injecting_input() {
    let mut script =
        InputScript::from_ron("(version:2,events:[(tick:0,action:Press(Key(Space)))])").unwrap();
    let mut sim = Sim::new(|_| {});
    assert!(catch_unwind(AssertUnwindSafe(|| sim.run_script(&script, 1))).is_err());
    assert_eq!(sim.current_tick(), 0);
    script.events.clear();
    script.version = 1;
    sim.run_script(&script, 1);
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Space));
}

#[test]
fn invalid_dt_and_missing_time_plugin_fail_early() {
    for dt in [0.0, -0.01, f64::INFINITY, f64::NAN, 1e-15, f64::MAX] {
        assert!(catch_unwind(AssertUnwindSafe(|| Sim::new(|_| {}).with_fixed_dt(dt))).is_err());
    }
    assert!(catch_unwind(AssertUnwindSafe(|| Sim::from_app(App::new()))).is_err());
}

#[test]
fn fixed_input_edges_are_per_frame_not_per_fixed_step() {
    for (fixed_ms, expected_presses) in [(40, 0), (10, 2)] {
        let mut sim = Sim::new(|app| {
            app.init_resource::<FixedCount>().add_systems(
                FixedUpdate,
                |keys: Res<ButtonInput<KeyCode>>, mut count: ResMut<FixedCount>| {
                    if keys.just_pressed(KeyCode::Space) {
                        count.0 += 1;
                    }
                },
            );
        })
        .with_fixed_dt(0.02);
        sim.world_mut()
            .resource_mut::<Time<Fixed>>()
            .set_timestep(Duration::from_millis(fixed_ms));
        sim.tap(KeyCode::Space);
        sim.run_ticks(3);
        assert_eq!(sim.resource::<FixedCount>().0, expected_presses);
    }
}

#[test]
fn consecutive_taps_and_tapping_a_held_key_follow_real_button_edge_semantics() {
    let mut sim = Sim::new(|_| {});
    sim.tap(KeyCode::Space);
    sim.tap(KeyCode::Space);
    let keys = sim.resource::<ButtonInput<KeyCode>>();
    assert!(keys.pressed(KeyCode::Space));
    assert!(keys.just_pressed(KeyCode::Space));
    assert!(keys.just_released(KeyCode::Space));
    sim.tick();
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Space));

    sim.press(KeyCode::Space);
    sim.tick();
    sim.tap(KeyCode::Space);
    assert!(!sim
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::Space));
    sim.tick();
    assert!(sim
        .resource::<ButtonInput<KeyCode>>()
        .just_released(KeyCode::Space));
}

#[derive(Resource, Default)]
struct Hooks {
    finish: u32,
    cleanup: u32,
}

struct HooksPlugin;

impl Plugin for HooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hooks>();
    }

    fn finish(&self, app: &mut App) {
        app.world_mut().resource_mut::<Hooks>().finish += 1;
    }

    fn cleanup(&self, app: &mut App) {
        app.world_mut().resource_mut::<Hooks>().cleanup += 1;
    }
}

#[test]
fn from_app_does_not_repeat_completed_plugin_hooks() {
    for already_cleaned in [false, true] {
        let mut app = App::new();
        app.add_plugins((bevy_time::TimePlugin, HooksPlugin));
        app.finish();
        if already_cleaned {
            app.cleanup();
        }
        let sim = Sim::from_app(app);
        assert_eq!(sim.resource::<Hooks>().finish, 1);
        assert_eq!(sim.resource::<Hooks>().cleanup, 1);
    }
}

struct NotReady;

impl Plugin for NotReady {
    fn build(&self, _: &mut App) {}

    fn ready(&self, _: &App) -> bool {
        false
    }
}

#[test]
fn from_app_rejects_plugins_that_are_not_ready() {
    let mut app = App::new();
    app.add_plugins((bevy_time::TimePlugin, NotReady));
    let message = panic_text(
        catch_unwind(AssertUnwindSafe(|| Sim::from_app(app)))
            .err()
            .unwrap(),
    );
    assert!(message.contains("plugins are not ready at tick 0"));
}

#[derive(Component)]
struct Player;

fn panic_text(payload: Box<dyn core::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        payload.downcast_ref::<&str>().unwrap().to_string()
    }
}

#[test]
fn failed_single_reports_filter_tick_matching_entities_and_names() {
    let mut sim = Sim::new(|_| {});
    let first = sim.world_mut().spawn((Player, Name::new("Alice"))).id();
    let second = sim.world_mut().spawn((Player, Name::new("Bob"))).id();
    sim.run_ticks(3);
    let error = catch_unwind(AssertUnwindSafe(|| {
        sim.single::<Entity, With<Player>>();
    }))
    .unwrap_err();
    let message = panic_text(error);
    for expected in ["tick 3", "Player", "2 entities matched", "Alice", "Bob"] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(message.contains(&first.to_string()), "{message}");
    assert!(message.contains(&second.to_string()), "{message}");
}

#[test]
fn timeout_reports_tick_condition_and_named_entities() {
    let mut sim = Sim::new(|_| {});
    let entity = sim.world_mut().spawn((Player, Name::new("Player"))).id();
    sim.run_ticks(10);
    let error = catch_unwind(AssertUnwindSafe(|| {
        sim.run_until_named(3, "Player reaches the goal", |_| false);
    }))
    .unwrap_err();
    let message = panic_text(error);
    for expected in [
        "after 3 ticks",
        "tick 13",
        "Player reaches the goal",
        "Player",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(message.contains(&entity.to_string()), "{message}");
}
