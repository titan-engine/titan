//! Exercise diagnostics with worker threads and nested schedule spans.

use bevy_app::{App, Update};
use bevy_ecs::{
    error::{ignore, BevyError, FallbackErrorHandler},
    prelude::*,
    schedule::{MultiThreadedExecutor, ScheduleLabel},
};
use std::{
    fs,
    sync::Mutex,
    thread::{self, ThreadId},
};
use titan_diagnostics::{DiagnosticReport, DiagnosticsLayer, DiagnosticsPlugin};
use tracing_subscriber::prelude::*;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Nested;

#[derive(Resource, Default)]
struct ObservedThreads(Mutex<Vec<ThreadId>>);

fn fail_one(threads: Res<ObservedThreads>) -> Result {
    threads.0.lock().unwrap().push(thread::current().id());
    tracing::warn!("worker warning");
    Err(BevyError::error("one"))
}
fn fail_two(threads: Res<ObservedThreads>) -> Result {
    threads.0.lock().unwrap().push(thread::current().id());
    Err(BevyError::error("two"))
}
fn nested(world: &mut World) {
    world.run_schedule(Nested);
}

#[test]
fn worker_errors_and_nested_schedules_are_attributed() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(DiagnosticsLayer))
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.insert_resource(FallbackErrorHandler(ignore));
    app.init_resource::<ObservedThreads>();
    let caller = thread::current().id();
    app.add_plugins(DiagnosticsPlugin {
        directory: directory.path().into(),
        ..Default::default()
    });
    app.add_systems(
        Update,
        (fail_one, fail_two, nested.after(fail_one).after(fail_two)),
    );
    app.add_systems(Nested, fail_one);
    app.edit_schedule(Update, |schedule| {
        schedule
            .set_executor(MultiThreadedExecutor::new())
            .set_apply_final_deferred(true);
    });
    app.update();
    assert!(
        app.world()
            .resource::<ObservedThreads>()
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|thread| *thread != caller),
        "the test must execute a detached worker, not a single-threaded task pool"
    );
    let saved: Vec<DiagnosticReport> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
        .collect();
    assert_eq!(saved.len(), 3);
    for (name, schedule) in [
        ("fail_one", "Update"),
        ("fail_two", "Update"),
        ("fail_one", "Nested"),
    ] {
        assert!(
            saved.iter().any(|report| {
                report.context.as_ref().unwrap().name.ends_with(name)
                    && report.schedule.as_deref() == Some(schedule)
                    && report.frame == Some(0)
            }),
            "{saved:?}"
        );
    }
    assert!(saved.iter().any(|report| report
        .recent_logs
        .iter()
        .any(|log| log.fields["message"] == "worker warning")));
}
