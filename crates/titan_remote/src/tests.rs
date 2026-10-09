use super::*;
use bevy_app::{FixedUpdate, Update};
use bevy_time::Fixed;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TimePlugin,
        FrameCountPlugin,
        RemotePlugin::default(),
        TitanRemotePlugin,
    ));
    app.finish();
    app.cleanup();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        20,
    )));
    app.update();
    app
}

fn call(app: &mut App, method: &str, params: Option<Value>) -> BrpResult {
    let handler = *app.world().resource::<RemoteMethods>().get(method).unwrap();
    let RemoteMethodSystemId::Instant(id) = handler else {
        panic!("Expected instant method");
    };
    app.world_mut().run_system_with(id, params).unwrap()
}

#[test]
fn pause_resume_and_status() {
    let mut app = app();
    assert_eq!(
        call(&mut app, "titan.status", None).unwrap(),
        json!({"paused":false,"frame":1,"pending_steps":0})
    );
    assert_eq!(call(&mut app, "titan.pause", None).unwrap()["paused"], true);
    let elapsed = app.world().resource::<Time<Virtual>>().elapsed();
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<Time<Virtual>>().elapsed(), elapsed);
    assert_eq!(call(&mut app, "titan.status", None).unwrap()["frame"], 4);
    assert_eq!(
        call(&mut app, "titan.resume", None).unwrap()["paused"],
        false
    );
    app.update();
    assert_eq!(
        app.world().resource::<Time<Virtual>>().elapsed() - elapsed,
        Duration::from_millis(20)
    );
}

#[test]
fn ten_frames_advance_exactly_and_restore_configuration() {
    let mut app = app();
    let dt = 0.02_f32;
    let duration = Duration::from_secs_f32(dt);
    {
        let mut time = app.world_mut().resource_mut::<Time<Virtual>>();
        time.set_relative_speed_f64(0.25);
        time.set_max_delta(Duration::from_millis(1));
    }
    call(&mut app, "titan.pause", None).unwrap();
    let elapsed = app.world().resource::<Time<Virtual>>().elapsed();
    let frame = app.world().resource::<FrameCount>().0;
    assert_eq!(
        call(
            &mut app,
            "titan.step",
            Some(json!({"frames":10,"dt_secs":dt}))
        )
        .unwrap(),
        json!({"target_frame":frame+10})
    );
    for n in 1..=10 {
        app.update();
        assert_eq!(app.world().resource::<FrameCount>().0, frame + n);
        assert_eq!(
            app.world().resource::<Time<Virtual>>().elapsed() - elapsed,
            duration * n
        );
        assert_eq!(
            call(&mut app, "titan.status", None).unwrap()["pending_steps"],
            10 - n
        );
    }
    let time = app.world().resource::<Time<Virtual>>();
    assert!(time.is_paused());
    assert_eq!(time.relative_speed_f64(), 0.25);
    assert_eq!(time.max_delta(), Duration::from_millis(1));
    assert!(
        matches!(app.world().resource::<TimeUpdateStrategy>(), TimeUpdateStrategy::ManualDuration(d) if *d == Duration::from_millis(20))
    );
    app.update();
    assert_eq!(
        app.world().resource::<Time<Virtual>>().elapsed() - elapsed,
        duration * 10
    );
}

#[test]
fn default_delta_zero_steps_overlap_and_cancellation() {
    let mut app = app();
    call(&mut app, "titan.step", Some(json!({"frames":0}))).unwrap();
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    call(&mut app, "titan.step", Some(json!({"frames":3}))).unwrap();
    assert_eq!(
        call(&mut app, "titan.step", Some(json!({"frames":1})))
            .unwrap_err()
            .code,
        STEP_IN_PROGRESS
    );
    app.update();
    assert_eq!(
        app.world().resource::<Time<Virtual>>().delta(),
        Duration::from_secs_f32(default_dt())
    );
    call(&mut app, "titan.pause", None).unwrap();
    assert!(app.world().resource::<StepState>().active.is_none());
    assert!(
        matches!(app.world().resource::<TimeUpdateStrategy>(), TimeUpdateStrategy::ManualDuration(d) if *d == Duration::from_millis(20))
    );
    call(&mut app, "titan.step", Some(json!({"frames":3}))).unwrap();
    app.update();
    call(&mut app, "titan.resume", None).unwrap();
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    assert!(app.world().resource::<StepState>().active.is_none());
}

#[test]
fn invalid_parameters_leave_state_unchanged() {
    let mut app = app();
    for params in [
        None,
        Some(json!({})),
        Some(json!({"frames":-1})),
        Some(json!({"frames":1.5})),
        Some(json!([1, 0.1])),
        Some(json!({"frames":4294967296_u64})),
        Some(json!({"frames":1,"dt_secs":0})),
        Some(json!({"frames":1,"dt_secs":-0.1})),
        Some(json!({"frames":1,"dt_secs":2})),
        Some(json!({"frames":1,"dt_secs":1e-30})),
        Some(json!({"frames":1,"unexpected":true})),
    ] {
        assert_eq!(
            call(&mut app, "titan.step", params).unwrap_err().code,
            error_codes::INVALID_PARAMS
        );
    }
    for method in ["titan.status", "titan.pause", "titan.resume"] {
        assert_eq!(
            call(&mut app, method, Some(json!({"unexpected":true})))
                .unwrap_err()
                .code,
            error_codes::INVALID_PARAMS
        );
    }
    assert!(app.world().resource::<StepState>().active.is_none());
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
}

#[test]
fn restores_other_time_update_strategies() {
    let mut app = app();
    let instant = app
        .world()
        .resource::<Time<bevy_time::Real>>()
        .last_update()
        .unwrap();
    for strategy in [
        TimeUpdateStrategy::Automatic,
        TimeUpdateStrategy::ManualInstant(instant),
        TimeUpdateStrategy::FixedTimesteps(2),
    ] {
        let kind = mem::discriminant(&strategy);
        app.insert_resource(strategy);
        call(&mut app, "titan.step", Some(json!({"frames":1}))).unwrap();
        app.update();
        let restored = app.world().resource::<TimeUpdateStrategy>();
        assert_eq!(mem::discriminant(restored), kind);
        match restored {
            TimeUpdateStrategy::ManualInstant(value) => assert_eq!(*value, instant),
            TimeUpdateStrategy::FixedTimesteps(value) => assert_eq!(*value, 2),
            _ => {}
        }
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
    }
}

#[test]
fn target_frame_wraps() {
    let mut app = app();
    app.world_mut().resource_mut::<FrameCount>().0 = u32::MAX;
    assert_eq!(
        call(&mut app, "titan.step", Some(json!({"frames":2}))).unwrap()["target_frame"],
        1
    );
    app.update();
    app.update();
    assert_eq!(app.world().resource::<FrameCount>().0, 1);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
}

#[derive(Resource, Default)]
struct Counts {
    fixed: u32,
    updates: u32,
}

#[test]
fn pausing_freezes_fixed_update_but_not_ordinary_systems() {
    let mut app = app();
    app.init_resource::<Counts>()
        .insert_resource(Time::<Fixed>::from_duration(Duration::from_millis(10)))
        .add_systems(FixedUpdate, |mut counts: ResMut<Counts>| {
            counts.fixed += 1;
        })
        .add_systems(Update, |mut counts: ResMut<Counts>| {
            counts.updates += 1;
        });
    call(&mut app, "titan.pause", None).unwrap();
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Counts>().fixed, 0);
    assert_eq!(app.world().resource::<Counts>().updates, 2);
    call(
        &mut app,
        "titan.step",
        Some(json!({"frames":2,"dt_secs":0.02})),
    )
    .unwrap();
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Counts>().fixed, 4);
}

#[test]
fn registration_preserves_builtin_methods_in_either_plugin_order() {
    for titan_first in [false, true] {
        let mut app = App::new();
        app.add_plugins((TimePlugin, FrameCountPlugin));
        if titan_first {
            app.add_plugins((TitanRemotePlugin, RemotePlugin::default()));
        } else {
            app.add_plugins((RemotePlugin::default(), TitanRemotePlugin));
        }
        app.finish();
        let methods = app.world().resource::<RemoteMethods>();
        assert!(methods.get("world.query").is_some());
        assert!(methods.get("titan.status").is_some());
    }
}
