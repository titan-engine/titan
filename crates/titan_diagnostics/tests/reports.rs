//! End-to-end diagnostics tests; fatal paths run only in child processes.

use bevy_app::{App, Update};
use bevy_diagnostic::FrameCount;
use bevy_ecs::{
    error::{BevyError, ErrorContext, FallbackErrorHandler},
    prelude::*,
    schedule::{ScheduleLabel, SingleThreadedExecutor},
};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, Once,
    },
};
use titan_diagnostics::{DiagnosticReport, DiagnosticsLayer, DiagnosticsPlugin, DiagnosticsState};
use tracing_subscriber::prelude::*;

static TEST_LOCK: Mutex<()> = Mutex::new(());
static SUBSCRIBER: Once = Once::new();
static FORWARDED: AtomicUsize = AtomicUsize::new(0);

fn previous(_: BevyError, _: ErrorContext) {
    FORWARDED.fetch_add(1, Ordering::SeqCst);
}

fn setup(directory: &Path, max_reports: usize, max_recent_logs: usize) -> App {
    SUBSCRIBER.call_once(|| {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(DiagnosticsLayer),
        )
        .unwrap();
    });
    let mut app = App::new();
    app.insert_resource(FallbackErrorHandler(previous));
    app.add_plugins(DiagnosticsPlugin {
        directory: directory.into(),
        max_reports,
        max_recent_logs,
        app_name: "test-game".into(),
        app_version: Some("1.2.3".into()),
    });
    app.edit_schedule(Update, |schedule| {
        schedule
            .set_executor(SingleThreadedExecutor::default())
            .set_apply_final_deferred(true);
    });
    app
}

fn reports(directory: &Path) -> Vec<DiagnosticReport> {
    let mut reports: Vec<DiagnosticReport> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| serde_json::from_slice(&fs::read(path).unwrap()).unwrap())
        .collect();
    reports.sort_by(|a, b| a.id.cmp(&b.id));
    reports
}

fn failing_system() -> Result {
    Err(BevyError::error("missing asset B0001"))
}

#[test]
fn system_context_frames_logs_and_deduplication() {
    let _lock = TEST_LOCK.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut app = setup(directory.path(), 4, 2);
    app.add_systems(Update, failing_system);
    FORWARDED.store(0, Ordering::SeqCst);
    tracing::info!("not retained");
    tracing::warn!("old warning");
    tracing::warn!(asset = "level.ron", "new warning");
    tracing::error!(entity = 42, "recent error");
    app.update();
    app.update();
    assert_eq!(FORWARDED.load(Ordering::SeqCst), 2);
    let saved = reports(directory.path());
    assert_eq!(saved.len(), 1);
    let report = &saved[0];
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.app_name, "test-game");
    assert_eq!(report.app_version.as_deref(), Some("1.2.3"));
    assert_eq!(report.kind, "error");
    assert_eq!(report.severity.as_deref(), Some("Error"));
    assert_eq!(report.schedule.as_deref(), Some("Update"));
    assert_eq!(report.first_frame, Some(0));
    assert_eq!(report.frame, Some(1));
    assert_eq!(report.count, 2);
    assert!(report.message.contains("missing asset B0001"));
    let context = report.context.as_ref().unwrap();
    assert_eq!(context.kind, "system");
    assert!(context.name.ends_with("failing_system"));
    assert!(context.last_run.is_some());
    assert_eq!(report.recent_logs.len(), 2);
    assert_eq!(report.recent_logs[0].fields["message"], "new warning");
    assert_eq!(report.recent_logs[0].fields["asset"], "level.ron");
    assert_eq!(report.recent_logs[1].level, "ERROR");
    assert!(app
        .world()
        .resource::<DiagnosticsState>()
        .last_write_error()
        .is_none());
    assert_eq!(
        fs::read_dir(directory.path()).unwrap().count(),
        1,
        "no temporary files left"
    );
}

fn failing_command(_: &mut World) -> Result {
    Err(BevyError::error("command failed"))
}
fn queue_command(mut commands: Commands) {
    commands.queue(failing_command);
}
#[derive(Event)]
struct FailureEvent;
fn failing_observer(_: On<FailureEvent>) -> Result {
    Err(BevyError::error("observer failed"))
}
fn trigger_observer(mut commands: Commands) {
    commands.trigger(FailureEvent);
}
fn failing_condition() -> Result<bool, BevyError> {
    Err(BevyError::error("condition failed"))
}
fn skipped_system() {
    panic!("condition should skip this system");
}

#[test]
fn command_observer_and_run_condition_context() {
    let _lock = TEST_LOCK.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut app = setup(directory.path(), 8, 0);
    app.add_observer(failing_observer);
    app.add_systems(
        Update,
        (
            queue_command,
            trigger_observer,
            skipped_system.run_if(failing_condition),
        ),
    );
    FORWARDED.store(0, Ordering::SeqCst);
    app.update();
    assert_eq!(FORWARDED.load(Ordering::SeqCst), 3);
    let saved = reports(directory.path());
    assert_eq!(saved.len(), 3);
    for (kind, name) in [
        ("command", "failing_command"),
        ("observer", "failing_observer"),
        ("run_condition", "failing_condition"),
    ] {
        let report = saved
            .iter()
            .find(|r| r.context.as_ref().is_some_and(|c| c.kind == kind))
            .unwrap();
        let context = report.context.as_ref().unwrap();
        assert!(context.name.contains(name), "{context:?}");
        assert_eq!(report.schedule.as_deref(), Some("Update"));
        assert_eq!(report.frame, Some(0));
        assert!(report.recent_logs.is_empty());
        if kind == "run_condition" {
            assert!(context.system.as_ref().unwrap().ends_with("skipped_system"));
            assert_eq!(context.on_set, Some(false));
        }
    }
}

#[test]
fn retention_preserves_unrelated_files_and_applies_across_runs() {
    let _lock = TEST_LOCK.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let unrelated = directory.path().join("titan-diagnostics-not-ours.json");
    fs::write(&unrelated, "leave me alone").unwrap();
    // Each new app is a new session, so records from disk are not deduped.
    for _ in 0..5 {
        let mut app = setup(directory.path(), 2, 0);
        app.add_systems(Update, failing_system);
        app.update();
    }
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "leave me alone");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
}

#[test]
fn io_failure_does_not_change_error_policy() {
    let _lock = TEST_LOCK.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("not-a-directory");
    fs::write(&file, "occupied").unwrap();
    let mut app = setup(&file, 2, 0);
    app.add_systems(Update, failing_system);
    FORWARDED.store(0, Ordering::SeqCst);
    app.update();
    assert_eq!(FORWARDED.load(Ordering::SeqCst), 1);
    assert!(app
        .world()
        .resource::<DiagnosticsState>()
        .last_write_error()
        .is_some());
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct CustomSchedule;

#[test]
fn custom_schedule_can_refresh_frame_explicitly() {
    let _lock = TEST_LOCK.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut app = setup(directory.path(), 4, 0);
    app.add_systems(CustomSchedule, failing_system);
    app.world_mut().resource_mut::<FrameCount>().0 = 123;
    titan_diagnostics::refresh_frame(app.world_mut());
    app.world_mut().run_schedule(CustomSchedule);
    let saved = reports(directory.path());
    assert_eq!(saved[0].schedule.as_deref(), Some("CustomSchedule"));
    assert_eq!(saved[0].frame, Some(123));
    assert_eq!(saved[0].first_frame, Some(123));
}

fn panicking_system() {
    panic!("deliberate system panic");
}

// This helper test only panics when explicitly launched by the parent process.
#[test]
fn child_process_entry() {
    let Some(directory) = std::env::var_os("TITAN_DIAGNOSTICS_CHILD_DIRECTORY") else {
        return;
    };
    let mode = std::env::var("TITAN_DIAGNOSTICS_CHILD_MODE").unwrap();
    std::panic::set_hook(Box::new(|_| {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "previous-panic-hook-ran");
    }));
    if mode == "second-app" {
        let _app = setup(Path::new(&directory), 8, 4);
        let _other = setup(Path::new(&directory), 8, 4);
    } else {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(DiagnosticsLayer),
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(FallbackErrorHandler(if mode == "broken-display" {
            |error, _| {
                assert!(error.is::<BrokenDisplay>());
                FORWARDED.fetch_add(1, Ordering::SeqCst);
            }
        } else {
            bevy_ecs::error::panic
        }));
        app.add_plugins(DiagnosticsPlugin {
            directory: directory.into(),
            ..Default::default()
        });
        app.edit_schedule(Update, |schedule| {
            schedule
                .set_executor(SingleThreadedExecutor::default())
                .set_apply_final_deferred(true);
        });
        if mode == "returned-error" {
            app.add_systems(Update, failing_system);
        } else if mode == "broken-display" {
            app.add_systems(Update, || -> Result { Err(BrokenDisplay.into()) });
        } else if mode == "unrelated-thread" {
            app.add_systems(Update, unrelated_thread_panic);
        } else {
            app.add_systems(Update, panic_as_result);
        }
        tracing::warn!("before panic");
        app.update();
        if mode == "broken-display" {
            assert_eq!(FORWARDED.load(Ordering::SeqCst), 1);
            assert!(app
                .world()
                .resource::<DiagnosticsState>()
                .last_write_error()
                .is_some());
        }
    }
}
#[derive(Debug)]
struct BrokenDisplay;
impl core::fmt::Display for BrokenDisplay {
    fn fmt(&self, _: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        panic!("broken error formatter");
    }
}
impl core::error::Error for BrokenDisplay {}

fn unrelated_thread_panic() {
    assert!(std::thread::spawn(|| {
        panic!("unrelated thread panic");
    })
    .join()
    .is_err());
}

fn panic_as_result() -> Result {
    panicking_system();
    Ok(())
}

#[test]
fn panics_write_reports_and_chain_hook_in_child_processes() {
    for mode in [
        "panic",
        "returned-error",
        "second-app",
        "broken-display",
        "unrelated-thread",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_process_entry", "--nocapture"])
            .env("TITAN_DIAGNOSTICS_CHILD_DIRECTORY", directory.path())
            .env("TITAN_DIAGNOSTICS_CHILD_MODE", mode)
            .env("RUST_BACKTRACE", "1")
            .output()
            .unwrap();
        if mode == "broken-display" {
            assert!(
                output.status.success(),
                "original error policy was changed: {output:?}"
            );
            continue;
        }
        if mode == "unrelated-thread" {
            assert!(
                output.status.success(),
                "parent app unexpectedly failed: {output:?}"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("previous-panic-hook-ran"));
            let saved = reports(directory.path());
            assert_eq!(saved.len(), 1);
            assert_eq!(saved[0].kind, "panic");
            assert!(saved[0].message.contains("unrelated thread panic"));
            assert!(saved[0].context.is_none());
            assert!(saved[0].schedule.is_none());
            assert_eq!(saved[0].frame, Some(0));
            continue;
        }
        assert!(
            !output.status.success(),
            "child unexpectedly succeeded: {output:?}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("previous-panic-hook-ran"));
        let saved = reports(directory.path());
        if mode == "panic" {
            let panic = saved.iter().find(|r| r.kind == "panic").unwrap();
            assert!(panic.message.contains("deliberate system panic"));
            assert_eq!(panic.schedule.as_deref(), Some("Update"));
            assert_eq!(panic.frame, Some(0));
            assert!(panic
                .context
                .as_ref()
                .unwrap()
                .name
                .contains("panic_as_result"));
            assert!(panic.backtrace.is_some());
            assert!(panic.location.is_some());
            assert!(panic
                .recent_logs
                .iter()
                .any(|log| log.fields["message"] == "before panic"));
            assert!(saved.iter().any(|r| r.kind == "error"
                && r.context
                    .as_ref()
                    .is_some_and(|c| c.name.contains("panic_as_result"))));
        } else if mode == "returned-error" {
            assert!(saved.iter().any(|r| r.kind == "error"
                && r.context
                    .as_ref()
                    .is_some_and(|c| c.name.contains("failing_system"))));
        } else {
            assert!(saved
                .iter()
                .any(|r| r.message.contains("only one live DiagnosticsPlugin")));
        }
    }
}
