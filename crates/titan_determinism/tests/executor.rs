//! A real executor-dependent failure using identical factories, without timing races.

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use titan_determinism::{DeterminismCheck, DeterminismReport, Variant};
use titan_test::Sim;

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Enemy {
    decision: u32,
}

#[derive(Resource)]
struct CallingThread(std::thread::ThreadId);

// BUG: gameplay reads an uncontrolled input (thread identity). These ambiguous
// writes also do not commute. Every sequential outcome is 1 or 2, whereas every
// worker-thread outcome is 10 or 20, so detection does not depend on which of
// the conflicting systems happens to run last.
fn choose_first(thread: Res<CallingThread>, mut enemies: Query<&mut Enemy>) {
    for mut enemy in &mut enemies {
        enemy.decision = if std::thread::current().id() == thread.0 {
            1
        } else {
            10
        };
    }
}

fn choose_second(thread: Res<CallingThread>, mut enemies: Query<&mut Enemy>) {
    for mut enemy in &mut enemies {
        enemy.decision = if std::thread::current().id() == thread.0 {
            2
        } else {
            20
        };
    }
}

#[test]
fn identical_factory_detects_worker_dependent_ambiguous_writes() {
    let caller = std::thread::current().id();
    let factory = || {
        Sim::new(|app| {
            app.register_type::<Enemy>()
                .insert_resource(CallingThread(caller))
                .add_systems(Update, (choose_first, choose_second));
            app.world_mut()
                .spawn((Name::new("Enemy"), Enemy { decision: 0 }));
        })
    };
    // The factory and entity allocation do not change between calls.
    DeterminismCheck::new(factory)
        .ticks(2)
        .run()
        .assert_deterministic();
    let report = DeterminismCheck::new(factory)
        .ticks(2)
        .variant(Variant::MultiThreaded)
        .run();
    let DeterminismReport::Diverged(divergence) = report else {
        panic!("the multithreaded executor must expose worker-dependent gameplay");
    };
    assert_eq!((divergence.run, divergence.tick), (2, 1));
    let enemy = &divergence.diff.entities[0];
    assert_eq!(enemy.after_name.as_deref(), Some("Enemy"));
    assert_eq!(enemy.components[0].fields[0].path, "$.decision");
    assert!(divergence.hints.ambiguities.iter().any(|hint| {
        hint.systems
            .iter()
            .any(|name| name.ends_with("::choose_first"))
            && hint
                .systems
                .iter()
                .any(|name| name.ends_with("::choose_second"))
    }));
}
