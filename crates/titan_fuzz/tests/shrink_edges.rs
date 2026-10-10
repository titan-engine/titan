//! Shrink confirmation, budget accounting, and replay endpoint regressions.

extern crate alloc;

use alloc::{string::String, vec::Vec};
use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput};
use core::sync::atomic::{AtomicU64, Ordering};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};
use titan_fuzz::{Failure, Fuzz, FuzzConfig, FuzzReport, Generator};
use titan_test::{InputAction, InputButton, Sim};

const DENSE: Generator = Generator {
    event_density: 1.0,
    min_hold_ticks: 1,
    max_hold_ticks: 1,
};

#[derive(Resource, Default)]
struct Frame(u64);

fn frame_sim() -> Sim {
    Sim::new(|app| {
        app.init_resource::<Frame>()
            .add_systems(Update, |mut frame: ResMut<Frame>| frame.0 += 1);
    })
}

fn transient_violation(world: &World) -> Result<(), String> {
    if world.resource::<Frame>().0 == 4 {
        Err("only broken on frame four".into())
    } else {
        Ok(())
    }
}

fn failed(report: &FuzzReport) -> &Failure {
    let FuzzReport::Failed(failure) = report else {
        panic!("expected reproducible failure, got {report:?}");
    };
    failure
}

#[test]
fn unconfirmed_truncation_still_displays_the_transient_replay_endpoint() {
    for budget in [0, 1] {
        let report = Fuzz::new(frame_sim)
            .buttons([KeyCode::Space])
            .generator(DENSE)
            .invariant("transient violation", transient_violation)
            .cases(1)
            .ticks(12)
            .max_shrink_runs(budget)
            .run();
        let failure = failed(&report);
        assert_eq!(failure.failing_tick, 3);
        assert_eq!(failure.ticks, 12);
        assert_eq!(failure.original_ticks, 12);
        assert_eq!(failure.shrink_runs, budget);
        assert_eq!(
            failure.script,
            DENSE.generate(&[KeyCode::Space.into()], 12, 0, 0)
        );

        let displayed = report.to_string();
        assert!(displayed.contains("sim.run_script(&script, 4);"));
        assert!(!displayed.contains("sim.run_script(&script, 12);"));
        let mut replay = frame_sim();
        replay.run_script(&failure.script, 4);
        assert_eq!(
            transient_violation(replay.world()),
            Err(failure.message.clone())
        );
        replay.run_script(&failure.script, failure.ticks);
        assert_eq!(transient_violation(replay.world()), Ok(()));
    }
}

#[derive(Clone, Copy, Resource)]
enum Confirmation {
    Pass,
    DifferentMessage,
    DifferentTick,
}

#[derive(Resource)]
struct RunNumber(u64);

#[test]
fn flaky_candidate_confirmation_restores_original_provenance_and_stops() {
    for confirmation in [
        Confirmation::Pass,
        Confirmation::DifferentMessage,
        Confirmation::DifferentTick,
    ] {
        let calls = AtomicU64::new(0);
        let report = Fuzz::new(|| {
            let run = calls.fetch_add(1, Ordering::SeqCst);
            Sim::new(|app| {
                app.init_resource::<Frame>()
                    .insert_resource(RunNumber(run))
                    .insert_resource(confirmation)
                    .add_systems(Update, |mut frame: ResMut<Frame>| frame.0 += 1);
            })
        })
        .buttons([KeyCode::Space, KeyCode::Space])
        .generator(DENSE)
        .invariant("transient violation", |world| {
            if world.resource::<RunNumber>().0 == 3 {
                match world.resource::<Confirmation>() {
                    Confirmation::Pass => return Ok(()),
                    Confirmation::DifferentMessage if world.resource::<Frame>().0 == 4 => {
                        return Err("different confirmation message".into());
                    }
                    Confirmation::DifferentTick if world.resource::<Frame>().0 == 2 => {
                        return Err("only broken on frame four".into());
                    }
                    _ => {}
                }
            }
            transient_violation(world)
        })
        .cases(7)
        .ticks(12)
        .seed(91)
        .max_shrink_runs(100)
        .test_name("candidate_confirmation")
        .output_dir("unused-fuzz-output")
        .run();
        let FuzzReport::Flaky(failure) = &report else {
            panic!("expected flaky candidate confirmation, got {report:?}");
        };
        let original = DENSE.generate(&[KeyCode::Space.into()], 12, 91, 0);
        assert_eq!(failure.script, original);
        assert_eq!(failure.original_events, original.events.len());
        assert_eq!(failure.ticks, 12);
        assert_eq!(failure.original_ticks, 12);
        assert_eq!(failure.failing_tick, 3);
        assert_eq!(failure.invariant, "transient violation");
        assert_eq!(failure.message, "only broken on frame four");
        assert_eq!(failure.seed, 91);
        assert_eq!(failure.case_index, 0);
        assert_eq!(
            failure.config,
            FuzzConfig {
                buttons: alloc::vec![KeyCode::Space.into()],
                generator: DENSE,
                cases: 7,
                ticks: 12,
                max_shrink_runs: 100,
            }
        );
        assert_eq!(
            failure.path,
            PathBuf::from("unused-fuzz-output")
                .join("candidate_confirmation")
                .join("transient_violation.ron")
        );
        assert_eq!(failure.shrink_runs, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
}

#[test]
fn every_factory_call_is_accounted_for_in_even_and_odd_shrink_budgets() {
    for budget in 0..=8 {
        let calls = AtomicU64::new(0);
        let report = Fuzz::new(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            frame_sim()
        })
        .buttons([KeyCode::Space])
        .generator(DENSE)
        .invariant("always broken", |world| {
            if world.resource::<Frame>().0 >= 4 {
                Err("broken regardless of input".into())
            } else {
                Ok(())
            }
        })
        .cases(9)
        .ticks(12)
        .max_shrink_runs(budget)
        .run();
        let failure = failed(&report);
        let actual_calls = calls.load(Ordering::SeqCst);
        assert_eq!(actual_calls, 2 + failure.shrink_runs, "budget {budget}");
        assert!(actual_calls <= 2 + budget, "budget {budget}");
        // A first candidate costs one run, but adopting it needs a second.
        if budget < 2 {
            assert_eq!(failure.ticks, 12);
        } else {
            assert_eq!(failure.ticks, 4);
        }
        assert_eq!(failure.failing_tick, 3);
        let mut replay = frame_sim();
        replay.run_script(&failure.script, failure.ticks);
        assert!(replay.resource::<Frame>().0 >= 4);
    }
}

fn input_panic_sim() -> Sim {
    Sim::new(|app| {
        app.add_systems(Update, |keys: Res<ButtonInput<KeyCode>>| {
            if keys.pressed(KeyCode::Space) {
                panic!("original input panic");
            }
            panic!("different panic after removing input");
        });
    })
}

#[test]
fn event_removal_cannot_replace_the_original_panic_with_a_different_panic() {
    let calls = AtomicU64::new(0);
    let report = Fuzz::new(|| {
        calls.fetch_add(1, Ordering::SeqCst);
        input_panic_sim()
    })
    .buttons([KeyCode::Space])
    .generator(DENSE)
    .cases(1)
    .ticks(12)
    .max_shrink_runs(30)
    .run();
    let failure = failed(&report);
    assert_eq!(failure.invariant, "no_panics");
    assert_eq!(failure.message, "original input panic");
    assert_eq!(failure.failing_tick, 0);
    assert_eq!(failure.ticks, 1);
    assert_eq!(failure.script.events.len(), 1);
    assert_eq!(
        failure.script.events[0].action,
        InputAction::Press(KeyCode::Space.into())
    );
    // Two truncation runs, then the rejected empty-script panic: no confirmation
    // of that different panic is allowed, even though its invariant name matches.
    assert_eq!(failure.shrink_runs, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 5);

    let mut replay = input_panic_sim();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        replay.run_script(&failure.script, failure.ticks);
    }))
    .expect_err("plain simulation must reproduce the original panic");
    let message = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
    assert_eq!(message, Some("original input panic"));
}

#[test]
fn generator_extreme_hold_limits_keep_pairs_ordered_and_bounded() {
    let buttons = [
        InputButton::from(KeyCode::Space),
        MouseButton::Left.into(),
        KeyCode::Space.into(),
    ];
    for generator in [
        DENSE,
        Generator {
            max_hold_ticks: u64::MAX,
            ..DENSE
        },
        Generator {
            min_hold_ticks: u64::MAX,
            max_hold_ticks: u64::MAX,
            ..DENSE
        },
    ] {
        for ticks in [0, 1, 2, 8] {
            let script = generator.generate(&buttons, ticks, u64::MAX, u64::MAX);
            assert_eq!(script.version, titan_test::SCRIPT_VERSION);
            assert!(script.events.windows(2).all(|p| p[0].tick <= p[1].tick));
            let mut held: Vec<(InputButton, u64)> = Vec::new();
            for event in &script.events {
                assert!(event.tick < ticks);
                match event.action {
                    InputAction::Press(button) => {
                        assert!(buttons.contains(&button));
                        assert!(!held.iter().any(|(other, _)| *other == button));
                        held.push((button, event.tick));
                    }
                    InputAction::Release(button) => {
                        let index = held
                            .iter()
                            .position(|(other, _)| *other == button)
                            .expect("every release must have a preceding press");
                        let (_, start) = held.remove(index);
                        let duration = event.tick - start;
                        assert!(duration >= generator.min_hold_ticks);
                        assert!(duration <= generator.max_hold_ticks);
                    }
                    InputAction::Tap(_)
                    | InputAction::ConnectGamepad { .. }
                    | InputAction::SetAxis { .. }
                    | InputAction::SetButtonValue { .. }
                    | InputAction::MouseMotion { .. } => {
                        panic!("generator must only produce press/release pairs")
                    }
                }
            }
            assert!(held.is_empty());
            if ticks < 2 || generator.min_hold_ticks >= ticks {
                assert!(script.events.is_empty());
            } else {
                assert!(!script.events.is_empty());
            }
        }
    }
}
