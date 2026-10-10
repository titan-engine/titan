//! End-to-end fuzz campaigns and plain `titan_test` replay regressions.

use bevy_app::{Startup, Update};
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput};
use bevy_math::{Quat, Vec3};
use bevy_transform::components::Transform;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use titan_fuzz::{Failure, Fuzz, FuzzReport, Generator};
use titan_test::{InputAction, InputButton, InputScript, Sim};

#[derive(Resource, Default)]
struct Player {
    x: i32,
    airborne_ticks: u8,
}

#[derive(Resource)]
struct SkipAirborneCollision(bool);

fn movement(
    keys: Res<ButtonInput<KeyCode>>,
    bug: Res<SkipAirborneCollision>,
    mut player: ResMut<Player>,
) {
    if keys.just_pressed(KeyCode::Space) {
        player.airborne_ticks = 6;
    }
    if keys.pressed(KeyCode::KeyD) {
        player.x += 1;
    }
    // Deliberate bug: jumping disables the wall collision.
    if !bug.0 || player.airborne_ticks == 0 {
        player.x = player.x.min(2);
    }
    player.airborne_ticks = player.airborne_ticks.saturating_sub(1);
}

fn game(bug: bool) -> Sim {
    Sim::new(|app| {
        app.init_resource::<Player>()
            .insert_resource(SkipAirborneCollision(bug))
            .add_systems(Update, movement);
    })
    .with_seed(7)
}

fn inside_wall(world: &World) -> Result<(), String> {
    let x = world.resource::<Player>().x;
    if x <= 2 {
        Ok(())
    } else {
        Err(format!("player crossed wall: x={x}"))
    }
}

fn campaign(bug: bool) -> FuzzReport {
    Fuzz::new(move || game(bug))
        .buttons([KeyCode::KeyD, KeyCode::Space])
        .invariant("inside wall", inside_wall)
        .no_nan_transforms()
        .generator(Generator {
            event_density: 0.35,
            min_hold_ticks: 1,
            max_hold_ticks: 10,
        })
        .cases(8)
        .ticks(128)
        .seed(1)
        .max_shrink_runs(300)
        .test_name("airborne_collision")
        .run()
}

fn failed(report: &FuzzReport) -> &Failure {
    match report {
        FuzzReport::Failed(failure) => failure,
        other => panic!("expected reproducible failure, got {other:?}"),
    }
}

#[test]
fn correct_game_passes() {
    let report = campaign(false);
    assert!(matches!(report, FuzzReport::Passed { cases: 8, .. }));
    report.assert_ok();
}

#[test]
fn finds_and_shrinks_airborne_collision_then_replays_without_fuzzer() {
    let report = campaign(true);
    let failure = failed(&report);
    assert_eq!(failure.invariant, "inside wall");
    assert_eq!(failure.original_ticks, 128);
    assert!(failure.script.events.len() * 4 < failure.original_events);
    assert!(failure.ticks * 4 < failure.original_ticks);
    assert!(failure.failing_tick < failure.ticks);
    assert!(failure.shrink_runs <= 300);

    let mut sim = game(true);
    sim.run_script(&failure.script, failure.ticks);
    assert_eq!(sim.current_tick(), failure.ticks);
    assert_eq!(inside_wall(sim.world()), Err(failure.message.clone()));

    // Plain playback can locate the same first failing zero-based tick too.
    let mut sim = game(true);
    for tick in 0..failure.ticks {
        sim.run_script(&failure.script, tick + 1);
        if inside_wall(sim.world()).is_err() {
            assert_eq!(tick, failure.failing_tick);
            break;
        }
    }
}

#[test]
fn same_seed_produces_identical_cases_and_shrunk_report() {
    let generator = Generator::default();
    let buttons = [KeyCode::Space.into(), MouseButton::Left.into()];
    let scripts: Vec<_> = (0..4)
        .map(|case| generator.generate(&buttons, 64, 123, case))
        .collect();
    for case in (0..4).rev() {
        assert_eq!(
            scripts[case],
            generator.generate(&buttons, 64, 123, case as u64)
        );
    }
    assert_eq!(campaign(true), campaign(true));
}

#[test]
fn generated_scripts_use_only_selected_buttons_and_stay_in_bounds() {
    let generator = Generator {
        event_density: 0.5,
        min_hold_ticks: 2,
        max_hold_ticks: 5,
    };
    let buttons = [InputButton::from(KeyCode::Space), MouseButton::Left.into()];
    for case in 0..12 {
        let script = generator.generate(&buttons, 32, 42, case);
        assert_eq!(script.version, 1);
        assert!(!script.events.is_empty());
        assert!(script
            .events
            .windows(2)
            .all(|pair| pair[0].tick <= pair[1].tick));
        for event in &script.events {
            assert!(event.tick < 32);
            let button = match event.action {
                InputAction::Press(button) | InputAction::Release(button) => button,
                InputAction::Tap(_) => panic!("generator should produce press/release events"),
            };
            assert!(buttons.contains(&button));
        }
        // Scripts are ordinary titan_test data, not a fuzzer-specific format.
        let ron = script.to_ron().unwrap();
        assert_eq!(InputScript::from_ron(&ron).unwrap(), script);
    }
    assert!(generator.generate(&buttons, 0, 42, 0).events.is_empty());
}

#[test]
fn shrinking_does_not_switch_to_a_different_invariant() {
    let report = Fuzz::new(|| Sim::new(|_| {}))
        .buttons([KeyCode::KeyD, KeyCode::Space])
        .generator(Generator {
            event_density: 1.0,
            min_hold_ticks: 1,
            max_hold_ticks: 1,
        })
        .invariant("both held", |world| {
            let keys = world.resource::<ButtonInput<KeyCode>>();
            if keys.pressed(KeyCode::KeyD) && keys.pressed(KeyCode::Space) {
                Err("both buttons held".into())
            } else {
                Ok(())
            }
        })
        .invariant("D must be held", |world| {
            if world
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyD)
            {
                Ok(())
            } else {
                Err("different failure after removing D".into())
            }
        })
        .cases(1)
        .ticks(16)
        .run();
    let failure = failed(&report);
    assert_eq!(failure.invariant, "both held");
    let mut replay = Sim::new(|_| {});
    replay.run_script(&failure.script, failure.ticks);
    let keys = replay.resource::<ButtonInput<KeyCode>>();
    assert!(keys.pressed(KeyCode::KeyD));
    assert!(keys.pressed(KeyCode::Space));
}

#[test]
fn no_panics_name_is_reserved() {
    assert!(std::panic::catch_unwind(|| {
        Fuzz::new(|| Sim::new(|_| {})).invariant("no_panics", |_| Ok(()));
    })
    .is_err());
}

#[test]
fn system_panic_becomes_no_panics_failure_with_message() {
    let report = Fuzz::new(|| {
        Sim::new(|app| {
            app.add_systems(Update, || panic!("deliberate gameplay panic"));
        })
    })
    .buttons([KeyCode::Space])
    .cases(1)
    .ticks(4)
    .max_shrink_runs(5)
    .run();
    let failure = failed(&report);
    assert_eq!(failure.invariant, "no_panics");
    assert!(failure.message.contains("deliberate gameplay panic"));
    assert_eq!(failure.failing_tick, 0);
}

#[derive(Resource)]
struct Unstable(bool);

#[test]
fn nonreproducing_factory_is_flaky_without_shrinking() {
    let calls = AtomicU64::new(0);
    let report = Fuzz::new(|| {
        let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
        Sim::new(|app| {
            app.insert_resource(Unstable(first));
        })
    })
    .buttons([KeyCode::Space])
    .invariant("stable world", |world| {
        if world.resource::<Unstable>().0 {
            Err("factory's first world is broken".into())
        } else {
            Ok(())
        }
    })
    .cases(10)
    .ticks(16)
    .max_shrink_runs(100)
    .run();
    let FuzzReport::Flaky(failure) = &report else {
        panic!("expected Flaky, got {report:?}");
    };
    assert_eq!(failure.invariant, "stable world");
    assert_eq!(failure.shrink_runs, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(report.to_string().to_lowercase().contains("flaky"));
}

#[test]
fn shrink_budget_is_respected_including_zero() {
    for budget in [0, 1, 3] {
        let report = Fuzz::new(|| Sim::new(|_| {}))
            .buttons([KeyCode::Space])
            .invariant("always broken", |_| Err("broken".into()))
            .cases(1)
            .ticks(64)
            .max_shrink_runs(budget)
            .run();
        let failure = failed(&report);
        assert!(failure.shrink_runs <= budget);
        let mut replay = Sim::new(|_| {});
        replay.run_script(&failure.script, failure.ticks);
        assert_eq!(failure.invariant, "always broken");
    }
}

#[derive(Resource, Default)]
struct Frame(u64);

#[test]
fn checks_every_tick_not_only_the_final_world() {
    let report = Fuzz::new(|| {
        Sim::new(|app| {
            app.init_resource::<Frame>()
                .add_systems(Update, |mut frame: ResMut<Frame>| frame.0 += 1);
        })
    })
    .buttons([KeyCode::Space])
    .invariant("transient violation", |world| {
        if world.resource::<Frame>().0 == 4 {
            Err("only broken on frame four".into())
        } else {
            Ok(())
        }
    })
    .cases(1)
    .ticks(12)
    .run();
    assert_eq!(failed(&report).failing_tick, 3);
    assert_eq!(failed(&report).ticks, 4);
}

#[test]
fn startup_has_run_before_first_invariant_check() {
    let report = Fuzz::new(|| {
        Sim::new(|app| {
            app.add_systems(Startup, |mut commands: Commands| {
                commands.insert_resource(Frame(1));
            });
        })
    })
    .buttons([KeyCode::Space])
    .invariant("startup resource exists", |world| {
        if world.get_resource::<Frame>().is_some() {
            Ok(())
        } else {
            Err("Startup has not run".into())
        }
    })
    .cases(1)
    .ticks(1)
    .run();
    assert_eq!(report, FuzzReport::Passed { cases: 1, ticks: 1 });
}

#[test]
fn finite_startup_transform_passes() {
    let report = Fuzz::new(|| {
        Sim::new(|app| {
            app.add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Transform::from_xyz(1.0, -2.0, 3.0));
            });
        })
    })
    .buttons([KeyCode::Space])
    .no_nan_transforms()
    .cases(1)
    .ticks(4)
    .run();
    assert_eq!(report, FuzzReport::Passed { cases: 1, ticks: 4 });
    assert!(ron::ser::to_string(&report).unwrap().contains("Passed"));
}

#[test]
fn no_nan_transforms_checks_all_translation_rotation_and_scale_fields_at_startup() {
    for index in 0..10 {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut fields = [0.0; 10];
            fields[6] = 1.0; // Quaternion w.
            fields[7..].fill(1.0); // Scale.
            fields[index] = value;
            let transform = Transform {
                translation: Vec3::new(fields[0], fields[1], fields[2]),
                rotation: Quat::from_xyzw(fields[3], fields[4], fields[5], fields[6]),
                scale: Vec3::new(fields[7], fields[8], fields[9]),
            };
            let report = Fuzz::new(move || {
                Sim::new(|app| {
                    app.add_systems(Startup, move |mut commands: Commands| {
                        commands.spawn(transform);
                    });
                })
            })
            .buttons([KeyCode::Space])
            .no_nan_transforms()
            .cases(1)
            .ticks(1)
            .max_shrink_runs(0)
            .run();
            let failure = failed(&report);
            assert_eq!(failure.invariant, "no_nan_transforms", "field {index}");
            assert_eq!(failure.failing_tick, 0, "field {index}");
        }
    }
}

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "titan-fuzz-test-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn report_serializes_and_assert_ok_saves_replay_before_panicking() {
    let directory = TestDirectory::new();
    let report = Fuzz::new(|| Sim::new(|_| {}))
        .buttons([KeyCode::Space])
        .invariant("player health", |_| Err("health exceeds maximum".into()))
        .cases(1)
        .ticks(4)
        .seed(123)
        .test_name("health_regression")
        .output_dir(&directory.0)
        .run();
    let failure = failed(&report);
    assert!(failure.path.starts_with(&directory.0));
    assert_eq!(failure.path.extension().unwrap(), "ron");
    assert_eq!(failure.seed, 123);
    assert_eq!(failure.case_index, 0);
    assert_eq!(failure.config.buttons, [InputButton::from(KeyCode::Space)]);
    assert_eq!(failure.config.cases, 1);
    assert_eq!(failure.config.ticks, 4);
    assert!(ron::ser::to_string(&report).unwrap().contains("Failed"));

    let displayed = report.to_string();
    assert!(displayed.contains("player health"));
    assert!(displayed.contains("health exceeds maximum"));
    assert!(displayed.contains("run_script"));
    assert!(displayed.contains("version: 1"));
    assert!(std::panic::catch_unwind(|| report.assert_ok()).is_err());
    let saved = std::fs::read_to_string(&failure.path).unwrap();
    assert_eq!(InputScript::from_ron(&saved).unwrap(), failure.script);
}

#[test]
fn flaky_assert_ok_also_saves_the_original_reproducer() {
    let directory = TestDirectory::new();
    let calls = AtomicU64::new(0);
    let report = Fuzz::new(|| {
        let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
        Sim::new(|app| {
            app.insert_resource(Unstable(first));
        })
    })
    .buttons([KeyCode::Space])
    .invariant("unstable", |world| {
        if world.resource::<Unstable>().0 {
            Err("unrepeatable".into())
        } else {
            Ok(())
        }
    })
    .cases(1)
    .ticks(8)
    .output_dir(&directory.0)
    .run();
    let FuzzReport::Flaky(failure) = &report else {
        panic!("expected Flaky, got {report:?}");
    };
    assert!(ron::ser::to_string(&report).unwrap().contains("Flaky"));
    assert!(std::panic::catch_unwind(|| report.assert_ok()).is_err());
    let saved = std::fs::read_to_string(&failure.path).unwrap();
    assert_eq!(InputScript::from_ron(&saved).unwrap(), failure.script);
}
