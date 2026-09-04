#![cfg(not(target_arch = "wasm32"))]
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use titan::{
    App, Name,
    inspection::{InspectionConfig, Inspector},
};
use titan_diagnostics::{DiagnosticBundle, DiagnosticInspector, DiagnosticPolicy, RequestHistory};
use titan_protocol::{Request, RequestEnvelope};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "titan-host-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn setup() -> (App, Inspector) {
    (
        App::new(),
        Inspector::new(InspectionConfig::controlled("test", "game")),
    )
}
#[test]
fn failure_bundle_preserves_response_and_records_recent_inputs() {
    let root = Temp::new();
    let (mut app, mut inspector) = setup();
    let entity = app.world_mut().spawn();
    app.world_mut().insert(entity, Name::new("player")).unwrap();
    inspector.register_input_handler(|_, _, _| Ok(()));
    let mut host = DiagnosticInspector::new(&root.0);
    let input = RequestEnvelope::new(
        "input",
        Request::InjectInput {
            frame: 1,
            actions: Default::default(),
        },
    );
    let accepted = host.handle(&mut inspector, &mut app, &input, |_, _| {
        panic!("success must not collect")
    });
    assert!(accepted.written.is_none());
    let failure = RequestEnvelope::new(
        "missing",
        Request::Invoke {
            name: "absent".into(),
            arguments: Default::default(),
        },
    );
    let result = host.handle(&mut inspector, &mut app, &failure, |_, bundle| {
        bundle.world_state["quest"] = serde_json::json!({"done": false});
        None
    });
    assert!(result.errors.is_empty());
    let written = result.written.unwrap();
    let bundle: DiagnosticBundle =
        serde_json::from_slice(&std::fs::read(&written.manifest).unwrap()).unwrap();
    assert_eq!(bundle.request, Some(failure));
    assert_eq!(bundle.history.requests.len(), 2);
    assert_eq!(bundle.history.accepted_inputs.len(), 1);
    assert_eq!(bundle.world_state["entities"][0]["name"], "player");
    assert_eq!(bundle.world_state["quest"]["done"], false);
    let response = bundle.response.unwrap();
    assert_eq!(response.observed_frame, result.response.observed_frame);
    assert_eq!(response.state_revision, result.response.state_revision);
    let returned = serde_json::to_value(result.response).unwrap();
    assert_eq!(
        returned["error"]["details"]["diagnostic_bundle"],
        written.manifest.to_str().unwrap()
    );
    assert!(bundle.timings_us.contains_key("request"));
    assert!(written.directory.join("api.txt").is_file());
}
#[test]
fn policy_and_failed_writes_do_not_change_game_results() {
    let root = Temp::new();
    let (mut app, mut inspector) = setup();
    let request = RequestEnvelope::new("status", Request::Status);
    let expected = inspector.handle(&mut app, &request);
    let mut host = DiagnosticInspector::new(&root.0);
    host.policy = DiagnosticPolicy::Always;
    host.history = RequestHistory::new(1, 4096);
    let result = host.handle(&mut inspector, &mut app, &request, |_, _| None);
    assert_eq!(result.response, expected);
    assert!(result.written.is_some());
    host.policy = DiagnosticPolicy::Never;
    assert!(
        host.handle(&mut inspector, &mut app, &request, |_, _| panic!("never"))
            .written
            .is_none()
    );
    assert_eq!(host.history.snapshot().dropped_entries, 1);
    let bad_root = root.0.join("file");
    std::fs::write(&bad_root, "not a directory").unwrap();
    let mut host = DiagnosticInspector::new(bad_root);
    host.policy = DiagnosticPolicy::Always;
    let result = host.handle(&mut inspector, &mut app, &request, |_, _| None);
    assert_eq!(result.response, expected);
    assert!(result.written.is_none());
    assert_eq!(result.errors.len(), 1);
}
#[test]
fn entity_snapshot_is_bounded_and_reports_truncation() {
    let root = Temp::new();
    let (mut app, mut inspector) = setup();
    let removed = app.world_mut().spawn();
    app.world_mut()
        .insert(removed, Name::new("removed"))
        .unwrap();
    app.world_mut().despawn(removed);
    for _ in 0..1001 {
        app.world_mut().spawn();
    }
    let mut host = DiagnosticInspector::new(&root.0);
    host.policy = DiagnosticPolicy::Always;
    let result = host.handle(
        &mut inspector,
        &mut app,
        &RequestEnvelope::new("status", Request::Status),
        |_, _| None,
    );
    let bundle: DiagnosticBundle =
        serde_json::from_slice(&std::fs::read(result.written.unwrap().manifest).unwrap()).unwrap();
    assert_eq!(
        bundle.world_state["entities"].as_array().unwrap().len(),
        1000
    );
    assert_eq!(bundle.world_state["entity_count"], 1001);
    assert_eq!(bundle.world_state["truncated"], true);
    assert!(
        bundle
            .api_summary
            .unwrap()
            .components
            .iter()
            .any(|component| component.name.ends_with("::Name"))
    );
}
