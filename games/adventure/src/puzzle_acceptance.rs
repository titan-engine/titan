//! Bounded room-one acceptance. Only adversarial setups use the feature-gated fixture.
use crate::game::{self, Action, Position};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use titan::{
    App, Startup,
    input::{ActionValue, InputTracker},
};

struct Run {
    app: App,
    input: InputTracker<Action>,
    trace: Vec<Value>,
}
impl Run {
    fn new() -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        game::select_room(&mut app, 1).unwrap();
        let trace = vec![game::status(&app)];
        Self {
            app,
            input: InputTracker::default(),
            trace,
        }
    }
    fn tick(&mut self, actions: &[Action], count: usize) {
        for _ in 0..count {
            self.app.world_mut().insert_resource(
                self.input
                    .sample(actions.iter().map(|a| (*a, ActionValue::PRESSED))),
            );
            self.app.advance_fixed(1);
            self.trace.push(game::status(&self.app));
        }
    }
    fn place(&mut self, index: usize, x: i32, y: i32, z: i32, grounded: bool) {
        game::fixture_set_character(&mut self.app, index, Position { x, y, z }, 0, grounded);
    }
    fn state(&self) -> &Value {
        self.trace.last().unwrap()
    }
    fn puzzle(&self) -> &Value {
        &self.state()["puzzle"]
    }
}
fn actions(value: &Value) -> Vec<Action> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|a| match a.as_str().unwrap() {
            "up" => Action::Up,
            "down" => Action::Down,
            "left" => Action::Left,
            "right" => Action::Right,
            "jump" => Action::Jump,
            "switch" => Action::Switch,
            "restart" => Action::Restart,
            _ => panic!("unknown action"),
        })
        .collect()
}
fn door(r: &Run, expected: &str) {
    assert_eq!(r.puzzle()["door"]["state"], expected);
}
fn fresh(r: &Run) {
    assert_eq!(r.puzzle()["complete"], false);
    door(r, "closed");
    for p in r.puzzle()["plates"].as_array().unwrap() {
        assert_eq!(p["pressed"], false);
    }
    assert_eq!(r.state()["active_character"], "jumper");
    assert_eq!(r.state()["session_tick"], 0);
    assert_eq!(r.state()["pending_inputs"], 0);
}

pub fn run() -> Value {
    let mut scenarios = BTreeMap::new();
    let segments: Value =
        serde_json::from_str(include_str!("../tests/puzzle-solution.json")).unwrap();
    let mut solution = Run::new();
    for segment in segments.as_array().unwrap() {
        solution.tick(
            &actions(&segment["actions"]),
            segment["ticks"].as_u64().unwrap() as usize,
        );
        match segment["checkpoint"].as_str() {
            Some("plate-a") => {
                assert_eq!(
                    solution.puzzle()["plates"][0]["occupants"],
                    json!(["jumper"])
                );
                door(&solution, "open_plate");
            }
            Some("plate-b") => {
                assert_eq!(
                    solution.puzzle()["plates"][0]["occupants"],
                    json!(["jumper"])
                );
                assert_eq!(
                    solution.puzzle()["plates"][1]["occupants"],
                    json!(["strong"])
                );
            }
            Some("exchange") => {
                assert_eq!(solution.puzzle()["plates"][0]["pressed"], false);
                door(&solution, "open_plate");
            }
            Some("jumper-exit") => {
                assert_eq!(
                    solution.puzzle()["exit"],
                    json!({"jumper":true,"strong":false})
                );
                assert_eq!(solution.puzzle()["complete"], false);
            }
            Some("complete") => {
                assert_eq!(solution.puzzle()["complete"], true);
                door(&solution, "closed");
            }
            _ => {}
        }
    }
    let recording = serde_json::to_value(game::recording(&solution.app).unwrap()).unwrap();
    let committed: Value =
        serde_json::from_str(include_str!("../tests/puzzle-recording.json")).unwrap();
    let canonical =
        serde_json::to_value(serde_json::from_value::<game::Recording>(committed.clone()).unwrap())
            .unwrap();
    assert_eq!(recording, canonical, "versioned solution recording drifted");
    let decoded: game::Recording = serde_json::from_value(committed).unwrap();
    let mut playback = Run::new();
    for (index, frame) in decoded.frames.iter().enumerate() {
        playback
            .app
            .world_mut()
            .insert_resource(frame.decode(&game::SCHEMA).unwrap());
        playback.app.advance_fixed(1);
        let state = game::status(&playback.app);
        assert_eq!(
            state,
            solution.trace[index + 1],
            "recorded playback tick {}",
            index + 1
        );
        playback.trace.push(state);
    }
    scenarios.insert("versioned-recording-every-tick", playback.trace);
    let expected = solution.state().clone();
    game::replay(
        &mut solution.app,
        serde_json::from_value(recording.clone()).unwrap(),
    )
    .unwrap();
    let replayed = game::status(&solution.app);
    for key in [
        "characters",
        "puzzle",
        "active_character",
        "session_tick",
        "consumed_input",
    ] {
        assert_eq!(replayed[key], expected[key], "solution replay {key}");
    }
    solution.trace.push(replayed);
    solution.tick(&[Action::Jump, Action::Left, Action::Switch], 8);
    for key in ["characters", "puzzle", "active_character", "session_tick"] {
        assert_eq!(
            solution.state()[key],
            expected[key],
            "completion freeze {key}"
        );
    }
    solution.tick(&[Action::Restart, Action::Jump, Action::Right], 1);
    fresh(&solution);
    solution.tick(&[Action::Jump, Action::Right], 3);
    assert_eq!(solution.state()["characters"]["jumper"]["x"], 1500);
    assert_eq!(solution.state()["characters"]["jumper"]["y"], 0);
    scenarios.insert("raw-solution-replay-latch-restart", solution.trace);

    let mut solo = Run::new();
    for segment in segments.as_array().unwrap().iter().take(4) {
        solo.tick(
            &actions(&segment["actions"]),
            segment["ticks"].as_u64().unwrap() as usize,
        );
    }
    solo.tick(&[Action::Down], 50);
    door(&solo, "closed");
    solo.tick(&[Action::Right, Action::Jump], 160);
    assert_eq!(solo.state()["characters"]["jumper"]["x"], 6800);
    assert_eq!(solo.puzzle()["complete"], false);
    scenarios.insert("solo-jumper-cannot-pass-full-height-door", solo.trace);

    let mut strong = Run::new();
    strong.tick(&[Action::Switch], 1);
    strong.tick(&[], 1);
    strong.tick(&[Action::Left], 25);
    strong.tick(&[Action::Up], 50);
    strong.tick(&[Action::Up, Action::Jump], 60);
    assert_eq!(strong.state()["characters"]["strong"]["y"], 0);
    assert_eq!(strong.state()["characters"]["strong"]["z"], 3200);
    door(&strong, "closed");
    scenarios.insert("strong-cannot-reach-plate-a", strong.trace);

    for (label, x, y, z, expected) in [
        ("min-boundary", 1700, 1000, 1700, true),
        ("max-boundary", 2300, 1000, 2300, true),
        ("outside-center", 2301, 1000, 2000, false),
        ("wrong-height", 2000, 0, 2000, false),
        ("airborne", 2000, 1200, 2000, false),
    ] {
        let mut r = Run::new();
        r.place(0, x, y, z, y <= 1000);
        r.tick(&[], 1);
        assert_eq!(r.puzzle()["plates"][0]["pressed"], expected, "{label}");
        scenarios.insert(label, r.trace);
    }
    for (label, x, y, z, expected) in [
        ("b-min-boundary", 9700, 0, 4700, true),
        ("b-max-boundary", 10300, 0, 5300, true),
        ("b-outside-center", 10301, 0, 5000, false),
        ("b-airborne", 10000, 500, 5000, false),
    ] {
        let mut r = Run::new();
        r.place(0, x, y, z, y == 0);
        r.tick(&[], 1);
        assert_eq!(r.puzzle()["plates"][1]["pressed"], expected, "{label}");
        scenarios.insert(label, r.trace);
    }
    let mut shared = Run::new();
    shared.place(0, 10000, 0, 5000, true);
    shared.place(1, 10000, 0, 5000, true);
    shared.tick(&[], 1);
    assert_eq!(
        shared.puzzle()["plates"][1]["occupants"],
        json!(["jumper", "strong"])
    );
    shared.tick(&[Action::Jump], 1);
    assert_eq!(shared.puzzle()["plates"][1]["occupants"], json!(["strong"]));
    door(&shared, "open_plate");
    scenarios.insert("shared-plate-grounded-occupants-only", shared.trace);
    // Movement on the opening tick still sees the closed door.
    let mut timing = Run::new();
    timing.tick(&[Action::Switch], 1);
    timing.tick(&[], 1);
    timing.place(0, 2000, 1000, 2000, true);
    timing.place(1, 6800, 0, 5000, true);
    timing.tick(&[Action::Right], 1);
    assert_eq!(timing.state()["characters"]["strong"]["x"], 6800);
    door(&timing, "open_plate");
    timing.tick(&[Action::Right], 1);
    assert_eq!(timing.state()["characters"]["strong"]["x"], 6860);
    scenarios.insert("door-opens-for-next-tick-collision", timing.trace);
    for (label, x, y, z, expected) in [
        ("grounded-obstruction", 7500, 0, 5000, "open_obstructed"),
        ("airborne-obstruction", 7500, 1500, 5000, "open_obstructed"),
        ("edge-only-clear", 6800, 0, 5000, "closed"),
    ] {
        let mut r = Run::new();
        r.place(0, 2000, 1000, 2000, true);
        r.tick(&[], 1);
        door(&r, "open_plate");
        r.place(0, 4000, 0, 5000, true);
        r.place(1, x, y, z, y == 0);
        r.tick(&[], 1);
        door(&r, expected);
        r.place(1, 8500, 0, 5000, true);
        r.tick(&[], 1);
        door(&r, "closed");
        scenarios.insert(label, r.trace);
    }
    for (label, x, y, z, complete) in [
        ("exit-full-boundary", 10200, 0, 1200, true),
        ("exit-partial-footprint", 10199, 0, 1200, false),
        ("exit-airborne", 10500, 500, 2000, false),
    ] {
        let mut r = Run::new();
        r.place(0, 11500, 0, 2000, true);
        r.place(1, x, y, z, y == 0);
        r.tick(&[], 1);
        assert_eq!(r.puzzle()["exit"]["jumper"], true);
        assert_eq!(r.puzzle()["complete"], complete);
        scenarios.insert(label, r.trace);
    }
    let mut staggered = Run::new();
    staggered.place(0, 10500, 0, 2000, true);
    staggered.tick(&[], 1);
    assert_eq!(staggered.puzzle()["complete"], false);
    staggered.place(0, 9500, 0, 2000, true);
    staggered.place(1, 11500, 0, 2000, true);
    staggered.tick(&[], 1);
    assert_eq!(staggered.puzzle()["complete"], false);
    scenarios.insert("exit-arrivals-must-overlap-in-time", staggered.trace);
    for fallen in [0, 1] {
        let mut r = Run::new();
        r.place(0, 2000, 1000, 2000, true);
        r.tick(&[], 1);
        door(&r, "open_plate");
        let mut inspector = game::configured_inspector(
            titan::inspection::InspectionConfig::controlled("puzzle-acceptance", "headless"),
        );
        let frame = r.state()["frame"].as_u64().unwrap() + 8;
        let response = inspector.handle(
            &mut r.app,
            &titan_protocol::RequestEnvelope::new(
                "pending-before-fall",
                titan_protocol::Request::InjectInput {
                    frame,
                    actions: BTreeMap::from([(
                        "restart".into(),
                        titan_protocol::InputValue::Button(true),
                    )]),
                },
            ),
        );
        assert!(matches!(
            response.outcome,
            titan_protocol::ResponseOutcome::Success { .. }
        ));
        assert_eq!(game::status(&r.app)["pending_inputs"], 1);
        let next_frame = r.state()["frame"].as_u64().unwrap() + 1;
        let response = inspector.handle(
            &mut r.app,
            &titan_protocol::RequestEnvelope::new(
                "held-before-fall",
                titan_protocol::Request::InjectInput {
                    frame: next_frame,
                    actions: BTreeMap::from([
                        ("right".into(), titan_protocol::InputValue::Button(true)),
                        ("jump".into(), titan_protocol::InputValue::Button(true)),
                    ]),
                },
            ),
        );
        assert!(matches!(
            response.outcome,
            titan_protocol::ResponseOutcome::Success { .. }
        ));
        r.place(fallen, 500, -2001, 6500, false);
        r.tick(&[Action::Right, Action::Jump], 1);
        fresh(&r);
        assert_eq!(r.state()["recovery_message_ticks"], 120);
        r.tick(&[Action::Right, Action::Jump], 4);
        assert_eq!(r.state()["characters"]["jumper"]["x"], 1500);
        scenarios.insert(
            if fallen == 0 {
                "recovery-jumper-resets-puzzle"
            } else {
                "recovery-inactive-strong-resets-puzzle"
            },
            r.trace,
        );
    }
    json!({"format_version":1,"recording":recording,"scenarios":scenarios})
}
#[cfg(test)]
mod tests {
    #[test]
    fn room_one_acceptance() {
        super::run();
    }
}
