//! Concurrent unrelated schedules must not replace the owner's schedule label.

extern crate alloc;

use alloc::sync::Arc;
use bevy_app::{App, Update};
use bevy_ecs::{
    error::{ignore, BevyError, FallbackErrorHandler},
    prelude::*,
    schedule::{MultiThreadedExecutor, Schedule, ScheduleLabel, SingleThreadedExecutor},
};
use std::{fs, sync::Barrier};
use titan_diagnostics::{DiagnosticReport, DiagnosticsLayer, DiagnosticsPlugin};
use tracing_subscriber::prelude::*;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Unrelated;

fn fail() -> Result {
    Err(BevyError::error("owner failed"))
}

#[test]
fn concurrent_schedule_uses_local_context_or_null_not_unrelated_context() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(DiagnosticsLayer))
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.insert_resource(FallbackErrorHandler(ignore));
    app.add_plugins(DiagnosticsPlugin {
        directory: directory.path().into(),
        ..Default::default()
    });
    app.add_systems(Update, fail);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let mut world = World::new();
            let mut schedule = Schedule::new(Unrelated);
            schedule.add_systems(move |_: &mut World| {
                entered.wait();
                release.wait();
            });
            schedule.run(&mut world);
        });
        entered.wait();
        app.edit_schedule(Update, |schedule| {
            schedule
                .set_executor(SingleThreadedExecutor::default())
                .set_apply_final_deferred(true);
        });
        app.update();
        app.edit_schedule(Update, |schedule| {
            schedule
                .set_executor(MultiThreadedExecutor::new())
                .set_apply_final_deferred(true);
        });
        app.update();
        release.wait();
    });
    let saved: Vec<DiagnosticReport> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
        .collect();
    assert!(saved.iter().any(
        |report| report.first_frame == Some(0) && report.schedule.as_deref() == Some("Update")
    ));
    assert!(saved.iter().all(|report| report.schedule.is_none() || report.schedule.as_deref() == Some("Update")), "{saved:?}");
    assert_eq!(saved.iter().map(|report| report.count).sum::<u64>(), 2);
}
