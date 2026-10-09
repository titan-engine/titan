//! Virtual time scaling and independently clocked sub-app regressions.

extern crate alloc;

use alloc::sync::Arc;
use bevy_app::{App, AppLabel, FixedUpdate, SubApp, Update};
use bevy_ecs::{prelude::*, schedule::ScheduleLabel};
use bevy_time::{Fixed, Real, Time, TimePlugin, TimeUpdateStrategy, Virtual};
use core::time::Duration;
use std::sync::Mutex;
use titan_test::Sim;

#[derive(Resource, Default)]
struct FixedCount(u64);

#[derive(Debug, Clone, PartialEq)]
struct Frame {
    real_delta: Duration,
    real_elapsed: Duration,
    virtual_delta: Duration,
    virtual_elapsed: Duration,
    speed: f64,
    paused: bool,
    fixed_step: Duration,
    fixed_updates: u64,
}

#[derive(Resource, Clone, Default)]
struct Frames(Arc<Mutex<Vec<Frame>>>);

fn timed_app(speed: f64, paused: bool) -> (App, Frames) {
    let mut app = App::new();
    let frames = Frames::default();
    app.add_plugins(TimePlugin)
        .insert_resource(frames.clone())
        .init_resource::<FixedCount>()
        .add_systems(FixedUpdate, |mut count: ResMut<FixedCount>| count.0 += 1)
        .add_systems(
            Update,
            |real: Res<Time<Real>>,
             virtual_time: Res<Time<Virtual>>,
             fixed: Res<Time<Fixed>>,
             count: Res<FixedCount>,
             frames: Res<Frames>| {
                frames.0.lock().unwrap().push(Frame {
                    real_delta: real.delta(),
                    real_elapsed: real.elapsed(),
                    virtual_delta: virtual_time.delta(),
                    virtual_elapsed: virtual_time.elapsed(),
                    speed: virtual_time.relative_speed_f64(),
                    paused: virtual_time.is_paused(),
                    fixed_step: fixed.timestep(),
                    fixed_updates: count.0,
                });
            },
        );
    let mut virtual_time = app.world_mut().resource_mut::<Time<Virtual>>();
    virtual_time.set_relative_speed_f64(speed);
    if paused {
        virtual_time.pause();
    }
    (app, frames)
}

#[test]
fn configured_speed_is_preserved_without_clamping_scaled_large_ticks() {
    for speed in [0.0, 0.5, 1.0, 2.0] {
        let (app, frames) = timed_app(speed, false);
        let mut sim = Sim::from_app(app).with_fixed_dt(0.5);
        sim.run_ticks(4);
        let frames = frames.0.lock().unwrap();
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(frame.real_delta, Duration::from_millis(500));
            assert_eq!(frame.virtual_delta, Duration::from_secs_f64(0.5 * speed));
            assert_eq!(
                frame.virtual_elapsed,
                Duration::from_secs_f64(0.5 * speed * (index + 1) as f64)
            );
            assert_eq!(frame.fixed_updates, (speed * (index + 1) as f64) as u64);
            assert_eq!(frame.speed, speed);
        }
    }
}

#[test]
fn paused_scaled_clock_can_resume_without_losing_time_to_the_cap() {
    let (app, frames) = timed_app(2.0, true);
    let mut sim = Sim::from_app(app).with_fixed_dt(0.5);
    sim.tick();
    assert!(sim.resource::<Time<Virtual>>().is_paused());
    assert_eq!(sim.resource::<Time<Virtual>>().relative_speed_f64(), 2.0);
    assert_eq!(
        sim.resource::<Time<Real>>().elapsed(),
        Duration::from_millis(500)
    );
    assert_eq!(sim.resource::<Time<Virtual>>().elapsed(), Duration::ZERO);
    assert_eq!(sim.resource::<FixedCount>().0, 0);
    sim.world_mut().resource_mut::<Time<Virtual>>().unpause();
    sim.tick();
    let frames = frames.0.lock().unwrap();
    assert_eq!(frames[0].virtual_delta, Duration::ZERO);
    assert_eq!(frames[1].virtual_delta, Duration::from_secs(1));
    assert_eq!(frames[1].fixed_updates, 2);
}

#[derive(AppLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Secondary;

fn replay_sub_app(use_setup_closure: bool) -> Vec<Frame> {
    let (mut secondary, frames) = timed_app(2.0, false);
    // These settings must be replaced, not retained from the supplied app.
    secondary.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(7)));
    secondary
        .world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(Duration::from_millis(3));
    let secondary = core::mem::take(secondary.main_mut());
    let mut sim = if use_setup_closure {
        Sim::new(|app| {
            app.insert_sub_app(Secondary, secondary);
        })
    } else {
        let (mut app, _) = timed_app(1.0, false);
        app.insert_sub_app(Secondary, secondary);
        Sim::from_app(app)
    }
    .with_fixed_dt(0.5);
    sim.run_ticks(4);
    frames.0.lock().unwrap().clone()
}

#[test]
fn setup_and_from_app_configure_sub_app_clocks_and_replay_identically() {
    for use_setup_closure in [false, true] {
        let first = replay_sub_app(use_setup_closure);
        assert_eq!(first, replay_sub_app(use_setup_closure));
        assert_eq!(first.len(), 4);
        for (index, frame) in first.iter().enumerate() {
            assert_eq!(frame.real_delta, Duration::from_millis(500));
            assert_eq!(
                frame.real_elapsed,
                Duration::from_millis(500) * (index as u32 + 1)
            );
            assert_eq!(frame.virtual_delta, Duration::from_secs(1));
            assert_eq!(frame.virtual_elapsed, Duration::from_secs(index as u64 + 1));
            assert_eq!(frame.fixed_step, Duration::from_millis(500));
            assert_eq!(frame.fixed_updates, 2 * (index as u64 + 1));
        }
    }
}

#[test]
fn default_timestep_primes_sub_app_time_without_a_hidden_update() {
    let (mut app, _) = timed_app(1.0, false);
    let (mut secondary, frames) = timed_app(1.0, false);
    app.insert_sub_app(Secondary, core::mem::take(secondary.main_mut()));
    let mut sim = Sim::from_app(app);
    assert!(frames.0.lock().unwrap().is_empty());
    sim.tick();
    let frames = frames.0.lock().unwrap();
    let dt = Duration::from_secs_f64(1.0 / 60.0);
    assert_eq!(frames[0].real_delta, dt);
    assert_eq!(frames[0].real_elapsed, dt);
    assert_eq!(frames[0].virtual_delta, dt);
    assert_eq!(frames[0].fixed_step, dt);
    assert_eq!(frames[0].fixed_updates, 1);
}

#[test]
fn paused_sub_app_retains_its_own_speed_and_pause_settings() {
    let (mut app, main_frames) = timed_app(2.0, false);
    let (mut secondary, sub_frames) = timed_app(0.5, true);
    app.insert_sub_app(Secondary, core::mem::take(secondary.main_mut()));
    let mut sim = Sim::from_app(app).with_fixed_dt(0.5);
    sim.run_ticks(2);
    let main = main_frames.0.lock().unwrap();
    let secondary = sub_frames.0.lock().unwrap();
    assert_eq!(main[1].virtual_elapsed, Duration::from_secs(2));
    assert_eq!(main[1].fixed_updates, 4);
    assert!(!main[1].paused);
    assert_eq!(secondary[1].real_elapsed, Duration::from_secs(1));
    assert_eq!(secondary[1].virtual_elapsed, Duration::ZERO);
    assert_eq!(secondary[1].fixed_updates, 0);
    assert_eq!(secondary[1].speed, 0.5);
    assert!(secondary[1].paused);
}

#[test]
fn already_updated_sub_app_keeps_elapsed_time_and_fixed_overstep() {
    let (mut app, _) = timed_app(1.0, false);
    let (mut secondary, frames) = timed_app(1.0, false);
    secondary.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        20,
    )));
    secondary
        .world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(Duration::from_millis(30));
    secondary.finish();
    secondary.cleanup();
    secondary.update();
    secondary.update();
    // The initial zero-delta frame and 20ms frame ran no 30ms fixed steps.
    assert_eq!(secondary.world().resource::<FixedCount>().0, 0);
    frames.0.lock().unwrap().clear();
    app.insert_sub_app(Secondary, core::mem::take(secondary.main_mut()));
    let mut sim = Sim::from_app(app).with_fixed_dt(0.02);
    sim.tick();
    let frames = frames.0.lock().unwrap();
    assert_eq!(frames[0].real_elapsed, Duration::from_millis(40));
    assert_eq!(frames[0].virtual_elapsed, Duration::from_millis(40));
    // Retaining the earlier 20ms overstep lets the new 20ms timestep run twice.
    assert_eq!(frames[0].fixed_updates, 2);
}

#[test]
fn clockless_sub_apps_remain_clockless() {
    let mut sim = Sim::new(|app| {
        let mut secondary = SubApp::new();
        secondary.update_schedule = Some(Update.intern());
        secondary.add_systems(Update, |world: &World| {
            assert!(!world.contains_resource::<Time<Real>>());
            assert!(!world.contains_resource::<Time<Virtual>>());
            assert!(!world.contains_resource::<Time<Fixed>>());
            assert!(!world.contains_resource::<TimeUpdateStrategy>());
        });
        app.insert_sub_app(Secondary, secondary);
    });
    sim.tick();
}
