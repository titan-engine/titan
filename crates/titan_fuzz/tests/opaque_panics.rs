//! Opaque panic payloads must not become unrelated minimized failures.

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, ButtonInput};
use core::sync::atomic::{AtomicU64, Ordering};
use std::panic::{catch_unwind, panic_any, AssertUnwindSafe};
use titan_fuzz::{Fuzz, FuzzReport, Generator};
use titan_test::Sim;

#[derive(Debug)]
struct OriginalPanic;

#[derive(Debug)]
struct OtherPanic;

const DENSE: Generator = Generator {
    event_density: 1.0,
    min_hold_ticks: 1,
    max_hold_ticks: 1,
};

fn input_panic_sim() -> Sim {
    Sim::new(|app| {
        app.add_systems(Update, |keys: Res<ButtonInput<KeyCode>>| {
            if keys.pressed(KeyCode::Space) {
                panic_any(OriginalPanic);
            }
            panic_any(OtherPanic);
        });
    })
}

#[test]
fn opaque_input_panic_preserves_original_script_without_shrinking() {
    let calls = AtomicU64::new(0);
    let report = Fuzz::new(|| {
        calls.fetch_add(1, Ordering::SeqCst);
        input_panic_sim()
    })
    .buttons([KeyCode::Space])
    .generator(DENSE)
    .cases(1)
    .ticks(12)
    .seed(42)
    .max_shrink_runs(100)
    .run();
    let FuzzReport::Failed(failure) = report else {
        panic!("expected opaque panic failure, got {report:?}");
    };
    assert_eq!(failure.invariant, "no_panics");
    assert!(failure.message.contains("non-string payload"));
    assert!(failure.message.contains("shrinking disabled"));
    assert_eq!(failure.failing_tick, 0);
    assert_eq!(failure.ticks, 12);
    assert_eq!(failure.shrink_runs, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        failure.script,
        DENSE.generate(&[KeyCode::Space.into()], 12, 42, 0)
    );
    let mut sim = input_panic_sim();
    let payload = catch_unwind(AssertUnwindSafe(|| {
        sim.run_script(&failure.script, failure.ticks);
    }))
    .expect_err("original script must still panic");
    assert!(payload.is::<OriginalPanic>());
}

#[test]
fn opaque_panic_type_changing_on_original_replay_is_flaky() {
    let calls = AtomicU64::new(0);
    let report = Fuzz::new(|| {
        let original = calls.fetch_add(1, Ordering::SeqCst) == 0;
        Sim::new(|app| {
            app.add_systems(Update, move || {
                if original {
                    panic_any(OriginalPanic);
                }
                panic_any(OtherPanic);
            });
        })
    })
    .buttons([KeyCode::Space])
    .generator(DENSE)
    .cases(1)
    .ticks(12)
    .max_shrink_runs(100)
    .run();
    let FuzzReport::Flaky(failure) = report else {
        panic!("expected changing opaque panic to be flaky, got {report:?}");
    };
    assert_eq!(failure.shrink_runs, 0);
    assert_eq!(failure.ticks, 12);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
