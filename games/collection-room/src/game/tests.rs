use super::*;
fn app() -> App {
    let mut app = build_game();
    app.update_schedule(Startup);
    app
}
fn move_ticks(app: &mut App, actions: &[Action], ticks: u64) {
    let mut tracker = InputTracker::default();
    app.world_mut()
        .insert_resource(tracker.sample(actions.iter().map(|a| (*a, ActionValue::PRESSED))));
    app.advance_fixed(ticks);
}
fn win(app: &mut App) {
    move_ticks(app, &[Action::Right], 8);
    move_ticks(app, &[Action::Up], 20);
    move_ticks(app, &[Action::Right], 16);
}
#[test]
fn winning_route_records_replays_and_retains_immutable_geometry() {
    let mut app = app();
    let initial = app
        .extracted::<Result<RenderFrame3d, Frame3dError>>()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(initial.draws().len(), 11);
    assert!(
        initial
            .draws()
            .windows(2)
            .all(|pair| pair[0].draw().order < pair[1].draw().order)
    );
    assert!(initial.camera().position().y > 0.0);
    win(&mut app);
    let state = status(&app);
    assert_eq!(state["position"], serde_json::json!({"x":3000,"z":-2000}));
    assert_eq!(state["collected"], 3);
    assert_eq!(state["completed"], true);
    assert_eq!(state["session_tick"], 44);
    assert_eq!(
        app.extracted::<Result<RenderFrame3d, Frame3dError>>()
            .unwrap()
            .as_ref()
            .unwrap()
            .draws()
            .len(),
        8
    );
    let recording = recording(&app).unwrap();
    replay(&mut app, recording).unwrap();
    let replayed = status(&app);
    for key in [
        "position",
        "collected",
        "completed",
        "remaining",
        "session_tick",
    ] {
        assert_eq!(state[key], replayed[key], "{key}");
    }
    assert_eq!(replayed["frame"], 88);
    assert_eq!(replayed["session_generation"], 1);
    assert_eq!(initial.draws().len(), 11);
    restart(&mut app);
    assert_eq!(initial.draws().len(), 11);
    assert_eq!(status(&app)["recorded_ticks"], 0);
    assert_eq!(status(&app)["session_generation"], 2);
    assert_eq!(status(&app)["completed"], false);
    assert_eq!(status(&app)["collected"], 0);
}
#[test]
fn collision_bounds_opposition_diagonal_and_duplicate_collection() {
    let mut app = app();
    move_ticks(
        &mut app,
        &[Action::Right, Action::Left, Action::Up, Action::Down],
        2,
    );
    assert_eq!(
        status(&app)["position"],
        serde_json::json!({"x":-3000,"z":3000})
    );
    move_ticks(&mut app, &[Action::Right, Action::Up], 1);
    assert_eq!(
        status(&app)["position"],
        serde_json::json!({"x":-2823,"z":2823})
    );
    let diagonal = (2.0_f64 * 177.0 * 177.0).sqrt();
    assert!((diagonal / 250.0 - 1.0).abs() < 0.002);
    restart(&mut app);
    move_ticks(&mut app, &[Action::Up], 12); // x=-3000,z=0
    move_ticks(&mut app, &[Action::Right], 30);
    assert_eq!(
        status(&app)["position"],
        serde_json::json!({"x":-1000,"z":0})
    );
    move_ticks(&mut app, &[Action::Left], 100);
    assert_eq!(status(&app)["position"]["x"], -4500);
    restart(&mut app);
    move_ticks(&mut app, &[Action::Right], 8);
    assert_eq!(status(&app)["collected"], 1);
    move_ticks(&mut app, &[], 20);
    assert_eq!(status(&app)["collected"], 1);
    win(&mut app); // Completion latch is tested independently below.
    restart(&mut app);
    win(&mut app);
    move_ticks(&mut app, &[Action::Down], 10);
    assert_eq!(status(&app)["completed"], true);
    assert_eq!(status(&app)["collected"], 3);
}
#[test]
fn restart_clears_scheduled_and_held_inputs_and_rejects_bad_replay_atomically() {
    use titan_protocol::{Request, RequestEnvelope, ResponseOutcome};
    let mut app = app();
    let mut inspector = configured_inspector(InspectionConfig::controlled("test", "room"));
    let response = inspector.handle(
        &mut app,
        &RequestEnvelope::new(
            "input",
            Request::InjectInput {
                frame: 2,
                actions: BTreeMap::from([("right".into(), InputValue::Button(true))]),
            },
        ),
    );
    assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
    restart(&mut app);
    app.advance_fixed(3);
    assert_eq!(
        status(&app)["position"],
        serde_json::json!({"x":-3000,"z":3000})
    );
    assert_eq!(status(&app)["pending_inputs"], 0);
    let mut recording = recording(&app).unwrap();
    recording.frames[0].active.push("unknown".into());
    let before = status(&app);
    assert!(replay(&mut app, recording).is_err());
    assert_eq!(status(&app), before);
}
#[test]
fn teleport_requires_opt_in_and_rejects_overflow_obstacles_and_unknown_arguments() {
    use titan_protocol::{Request, RequestEnvelope, ResponseOutcome};
    let mut app = app();
    let invoke = |args| {
        RequestEnvelope::new(
            "teleport",
            Request::Invoke {
                name: "teleport".into(),
                arguments: serde_json::from_value(args).unwrap(),
            },
        )
    };
    let mut inspector = configured_inspector(InspectionConfig::controlled("test", "room"));
    let before = status(&app);
    assert!(matches!(
        inspector
            .handle(&mut app, &invoke(serde_json::json!({"x":1000,"z":3000})))
            .outcome,
        ResponseOutcome::Failure { .. }
    ));
    assert_eq!(status(&app), before);
    let mut config = InspectionConfig::controlled("test", "room");
    config.mutation_enabled = true;
    let mut inspector = configured_inspector(config);
    for args in [
        serde_json::json!({"x":i32::MIN,"z":0}),
        serde_json::json!({"x":0,"z":0}),
        serde_json::json!({"x":1000,"z":3000,"oops":true}),
        serde_json::json!({"x":1.5,"z":3000}),
    ] {
        assert!(matches!(
            inspector.handle(&mut app, &invoke(args)).outcome,
            ResponseOutcome::Failure { .. }
        ));
        assert_eq!(status(&app), before);
    }
    assert!(matches!(
        inspector
            .handle(&mut app, &invoke(serde_json::json!({"x":1000,"z":3000})))
            .outcome,
        ResponseOutcome::Success { .. }
    ));
    assert!(recording(&app).is_err());
    restart(&mut app);
    assert!(recording(&app).is_ok());
}
#[test]
fn recording_is_bounded_and_truncated_replay_rejected() {
    let mut app = app();
    app.advance_fixed(MAX_RECORDING_TICKS as u64 + 1);
    assert_eq!(status(&app)["recorded_ticks"], MAX_RECORDING_TICKS);
    let recording = recording(&app).unwrap();
    assert!(recording.truncated);
    assert!(replay(&mut app, recording).is_err());
}
