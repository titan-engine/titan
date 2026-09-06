//! Complete input-only slice routes and boundary regressions on native and WASM.
use crate::game::{self, Action};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use titan::{
    App, Startup,
    input::{ActionValue, InputTracker},
};
use titan_protocol::{InputValue, Request, RequestEnvelope, ResponseOutcome};

struct Run {
    app: App,
    input: InputTracker<Action>,
    trace: Vec<Value>,
    injected: bool,
}
impl Run {
    fn new(injected: bool) -> Self {
        let mut app = game::build_game();
        app.update_schedule(Startup);
        let trace = vec![game::status(&app)];
        Self {
            app,
            input: InputTracker::default(),
            trace,
            injected,
        }
    }
    fn state(&self) -> Value {
        game::status(&self.app)
    }
    fn queue_stale_restart(&mut self) {
        let mut inspector = game::configured_inspector(
            titan::inspection::InspectionConfig::controlled("sequence-pending", "headless"),
        );
        let frame = self.state()["frame"].as_u64().unwrap() + 3;
        let response = inspector.handle(
            &mut self.app,
            &RequestEnvelope::new(
                "stale-restart",
                Request::InjectInput {
                    frame,
                    actions: BTreeMap::from([("restart".into(), InputValue::Button(true))]),
                },
            ),
        );
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        assert_eq!(self.state()["pending_inputs"], 1);
    }

    fn tick(&mut self, actions: &[Action], count: usize) {
        for _ in 0..count {
            if self.injected {
                let mut inspector =
                    game::configured_inspector(titan::inspection::InspectionConfig::controlled(
                        "sequence-acceptance",
                        "headless",
                    ));
                let response = inspector.handle(
                    &mut self.app,
                    &RequestEnvelope::new(
                        "sequence-input",
                        Request::InjectInput {
                            frame: self.trace.last().unwrap()["frame"].as_u64().unwrap() + 1,
                            actions: actions
                                .iter()
                                .map(|a| {
                                    (
                                        game::SCHEMA
                                            .iter()
                                            .find(|(v, _)| v == a)
                                            .unwrap()
                                            .1
                                            .to_string(),
                                        InputValue::Button(true),
                                    )
                                })
                                .collect(),
                        },
                    ),
                );
                assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
            } else {
                self.app.world_mut().insert_resource(
                    self.input
                        .sample(actions.iter().map(|a| (*a, ActionValue::PRESSED))),
                );
            }
            self.app.advance_fixed(1);
            self.trace.push(self.state());
        }
    }
    fn route(&mut self, source: &str) {
        let route: Value = serde_json::from_str(source).unwrap();
        for segment in route.as_array().unwrap() {
            let actions: Vec<_> = segment["actions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    game::SCHEMA
                        .iter()
                        .find(|(_, name)| *name == s.as_str().unwrap())
                        .unwrap()
                        .0
                })
                .collect();
            self.tick(&actions, segment["ticks"].as_u64().unwrap() as usize);
            match segment["checkpoint"].as_str() {
                Some("plate-a") => assert_eq!(self.state()["puzzle"]["plates"][0]["pressed"], true),
                Some("plate-b") => assert_eq!(self.state()["puzzle"]["plates"][1]["pressed"], true),
                Some("complete") => assert_eq!(self.state()["puzzle"]["complete"], true),
                _ => {}
            }
        }
    }
    fn fresh(&self, room: u8) {
        let s = self.state();
        assert_eq!(s["room"], room);
        assert_eq!(s["phase"], "playing");
        assert_eq!(s["active_character"], "jumper");
        assert_eq!(s["session_tick"], 0);
        assert_eq!(s["pending_inputs"], 0);
        assert_eq!(s["puzzle"]["complete"], false);
        assert_eq!(s["puzzle"]["door"]["state"], "closed");
        assert_eq!(s["characters"]["jumper"]["x"], 1500);
        assert_eq!(s["characters"]["jumper"]["y"], 0);
        if room == 2 {
            assert_eq!(s["block"]["socket"], 0);
        }
    }
}
fn semantic_equal(a: &Value, b: &Value) {
    for key in [
        "room",
        "phase",
        "characters",
        "puzzle",
        "block",
        "active_character",
        "session_tick",
        "consumed_input",
        "blocked_actions",
    ] {
        assert_eq!(a[key], b[key], "replay semantic field {key}");
    }
}
pub fn run() -> Value {
    let mut scenarios = BTreeMap::new();
    let mut recordings = BTreeMap::new();
    for (label, source) in [
        (
            "versioned-final",
            include_str!("../tests/sequence-solution.json"),
        ),
        (
            "versioned-intermediate",
            include_str!("../tests/sequence-intermediate-solution.json"),
        ),
    ] {
        let mut original = Run::new(false);
        original.route(source);
        assert_eq!(original.state()["phase"], "slice_complete");
        let recording = game::recording(&original.app).unwrap();
        let mut playback = Run::new(false);
        for (index, frame) in recording.frames.iter().enumerate() {
            playback
                .app
                .world_mut()
                .insert_resource(frame.decode(&game::SCHEMA).unwrap());
            playback.app.advance_fixed(1);
            let state = playback.state();
            assert_eq!(
                state,
                original.trace[index + 1],
                "complete recording tick {}",
                index + 1
            );
            playback.trace.push(state);
        }
        recordings.insert(label.to_string(), serde_json::to_value(recording).unwrap());
        scenarios.insert(label.to_string(), playback.trace);
    }

    for (label, source) in [
        (
            "intermediate",
            include_str!("../tests/block-intermediate-solution.json"),
        ),
        ("final", include_str!("../tests/block-solution.json")),
    ] {
        for injected in [false, true] {
            let name = format!("{label}-{}", if injected { "injected" } else { "sampled" });
            let mut r = Run::new(injected);
            assert_eq!(r.state()["phase"], "start");
            r.tick(
                &[
                    Action::Right,
                    Action::Jump,
                    Action::Switch,
                    Action::Interact,
                ],
                3,
            );
            assert_eq!(r.state()["characters"], r.trace[0]["characters"]);
            assert_eq!(r.state()["session_tick"], 0);
            r.tick(
                &[
                    Action::Confirm,
                    Action::Right,
                    Action::Jump,
                    Action::Switch,
                    Action::Interact,
                ],
                1,
            );
            r.fresh(1);
            let initial = r.state()["characters"].clone();
            r.tick(
                &[
                    Action::Confirm,
                    Action::Right,
                    Action::Jump,
                    Action::Switch,
                    Action::Interact,
                ],
                3,
            );
            assert_eq!(r.state()["characters"], initial);
            r.tick(&[], 1);
            r.route(include_str!("../tests/puzzle-solution.json"));
            assert_eq!(r.state()["phase"], "room_complete");
            let complete = r.state();
            r.tick(
                &[
                    Action::Right,
                    Action::Jump,
                    Action::Switch,
                    Action::Interact,
                ],
                3,
            );
            assert_eq!(r.state()["characters"], complete["characters"]);
            assert_eq!(r.state()["room"], 1);
            let generation = r.state()["session_generation"].as_u64().unwrap();
            if injected {
                r.queue_stale_restart();
            }
            r.tick(
                &[
                    Action::Confirm,
                    Action::Right,
                    Action::Jump,
                    Action::Switch,
                    Action::Interact,
                ],
                1,
            );
            r.fresh(2);
            assert_eq!(r.state()["session_generation"], generation + 1);
            let initial = r.state()["characters"].clone();
            r.tick(
                &[
                    Action::Confirm,
                    Action::Right,
                    Action::Jump,
                    Action::Switch,
                    Action::Interact,
                ],
                3,
            );
            assert_eq!(r.state()["characters"], initial);
            assert_eq!(r.state()["block"]["moves"], 0);
            r.tick(&[], 1);
            r.route(source);
            assert_eq!(r.state()["phase"], "slice_complete");
            assert_eq!(
                r.state()["block"]["socket"],
                if label == "intermediate" { 1 } else { 2 }
            );
            let complete = r.state();
            r.tick(&[Action::Right, Action::Jump, Action::Switch], 3);
            assert_eq!(r.state()["characters"], complete["characters"]);
            let recording = game::recording(&r.app).unwrap();
            let value = serde_json::to_value(&recording).unwrap();
            let expected = r.state();
            game::replay(&mut r.app, recording).unwrap();
            semantic_equal(&r.state(), &expected);
            r.trace.push(r.state());
            recordings.insert(name.clone(), value);
            // Play again starts the sequence in room 1; no accidental action survives.
            r.tick(&[], 1);
            r.tick(
                &[Action::Confirm, Action::Right, Action::Jump, Action::Switch],
                1,
            );
            r.fresh(1);
            let initial = r.state()["characters"].clone();
            r.tick(
                &[Action::Confirm, Action::Right, Action::Jump, Action::Switch],
                3,
            );
            assert_eq!(r.state()["characters"], initial);
            // Reach final completion again through raw inputs, then R restarts room 2.
            r.tick(&[], 1);
            r.route(include_str!("../tests/puzzle-solution.json"));
            r.tick(&[Action::Confirm], 1);
            r.fresh(2);
            r.tick(&[], 1);
            r.route(source);
            r.tick(
                &[
                    Action::Restart,
                    Action::Confirm,
                    Action::Jump,
                    Action::Right,
                ],
                1,
            );
            r.fresh(2);
            let generation = r.state()["session_generation"].clone();
            let initial = r.state()["characters"].clone();
            r.tick(
                &[
                    Action::Restart,
                    Action::Confirm,
                    Action::Jump,
                    Action::Right,
                ],
                3,
            );
            assert_eq!(r.state()["session_generation"], generation);
            assert_eq!(r.state()["characters"], initial);
            r.tick(&[], 1);
            r.tick(&[Action::Right, Action::Jump], 1);
            assert_eq!(r.state()["characters"]["jumper"]["x"], 1560);
            assert_eq!(r.state()["characters"]["jumper"]["y"], 170);
            scenarios.insert(name, r.trace);
        }
    }
    json!({"format_version":1,"recordings":recordings,"scenarios":scenarios})
}
#[cfg(test)]
mod tests {
    #[test]
    fn complete_slice() {
        super::run();
    }
}
