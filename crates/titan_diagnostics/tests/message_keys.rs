//! Regression tests for multiline/Unicode message deduplication.

use bevy_app::{App, Update};
use bevy_ecs::{
    error::{BevyError, FallbackErrorHandler},
    prelude::*,
};
use std::{
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};
use titan_diagnostics::{DiagnosticReport, DiagnosticsLayer, DiagnosticsPlugin};
use tracing_subscriber::prelude::*;

static RECEIVED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static STEP: AtomicUsize = AtomicUsize::new(0);

fn failing() -> Result {
    let step = STEP.fetch_add(1, Ordering::SeqCst);
    Err(BevyError::error(match step {
        0 | 1 => "a\r\nb\r\né\r\n0: detail",
        2 => "failure\n0: asset 1",
        3 => "failure\n0: asset 2",
        4 => "failure\n   0: asset 1",
        5 => "failure\n   0: asset 2",
        6 => "failure\n   0: levels/demo.ron",
        7 => "failure\n   0: levels/other.ron",
        8 => "failure\n   0: level_1",
        9 => "failure\n   0: level_2",
        10 => "failure\n   0: std::backtrace::Backtrace::capture\nfirst asset",
        11 => "failure\n   0: std::backtrace::Backtrace::capture\nsecond asset",
        12 => "failure\n   0: std::backtrace::Backtrace::capture\n   1: demo::first\n             at demo.rs:1:1",
        13 => "failure\n   0: std::backtrace::Backtrace::capture\n   1: demo::second\n             at demo.rs:2:1",
        14 => "failure\n   0: std::backtrace::Backtrace::capture\nfirst asset\nnote: Some \"noisy\" backtrace lines have been filtered out. Run with `BEVY_BACKTRACE=full` for a verbose backtrace.",
        _ => "failure\n   0: std::backtrace::Backtrace::capture\nsecond asset\nnote: Some \"noisy\" backtrace lines have been filtered out. Run with `BEVY_BACKTRACE=full` for a verbose backtrace.",
    }))
}

#[test]
fn unicode_crlf_and_numbered_messages_are_preserved() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(DiagnosticsLayer))
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.insert_resource(FallbackErrorHandler(|error, _| {
        RECEIVED.lock().unwrap().push(error.to_string());
    }));
    app.add_plugins(DiagnosticsPlugin {
        directory: directory.path().into(),
        ..Default::default()
    });
    app.add_systems(Update, failing);
    for _ in 0..16 {
        app.update();
    }
    let received = RECEIVED.lock().unwrap();
    assert_eq!(received.len(), 16);
    assert!(received[0].starts_with("a\r\nb\r\né\r\n0: detail"));
    assert!(received[1].starts_with("a\r\nb\r\né\r\n0: detail"));
    let saved: Vec<DiagnosticReport> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
        .collect();
    assert_eq!(saved.len(), 15);
    assert_eq!(saved.iter().filter(|report| report.count == 2).count(), 1);
    assert!(saved.iter().all(|report| report.kind == "error"));
}
