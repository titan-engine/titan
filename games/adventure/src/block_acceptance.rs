//! Room-two input solutions and adversarial fixtures, shared by native and actual WASM.
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
        game::select_room(&mut app, 2).unwrap();
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
    fn character(&self, name: &str) -> &Value {
        &self.state()["characters"][name]
    }
    fn strong(&mut self) {
        self.tick(&[Action::Switch], 1);
        self.tick(&[], 1);
    }
}
fn actions(v: &Value) -> Vec<Action> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|a| match a.as_str().unwrap() {
            "up" => Action::Up,
            "down" => Action::Down,
            "left" => Action::Left,
            "right" => Action::Right,
            "jump" => Action::Jump,
            "interact" => Action::Interact,
            "switch" => Action::Switch,
            "restart" => Action::Restart,
            _ => panic!("unknown action"),
        })
        .collect()
}
fn fresh(r: &Run) {
    assert_eq!(r.state()["room"], 2);
    assert_eq!(r.puzzle()["complete"], false);
    assert_eq!(r.puzzle()["door"]["state"], "closed");
    assert_eq!(r.state()["active_character"], "jumper");
    assert_eq!(r.state()["session_tick"], 0);
    assert_eq!(r.state()["pending_inputs"], 0);
    assert_eq!(r.state()["block"]["socket"], 0);
    assert_eq!(r.state()["block"]["moves"], 0);
    assert_eq!(r.state()["block"]["last_rejection"], Value::Null);
    assert_eq!(r.character("jumper")["x"], 1500);
    assert_eq!(r.character("strong")["x"], 3500);
}
pub fn run() -> Value {
    let mut scenarios = BTreeMap::new();
    let mut recordings = BTreeMap::new();
    for (label, source) in [
        (
            "intermediate",
            include_str!("../tests/block-intermediate-solution.json"),
        ),
        ("final", include_str!("../tests/block-solution.json")),
    ] {
        let segments: Value = serde_json::from_str(source).unwrap();
        let mut r = Run::new();
        for segment in segments.as_array().unwrap() {
            r.tick(
                &actions(&segment["actions"]),
                segment["ticks"].as_u64().unwrap() as usize,
            );
            match segment["checkpoint"].as_str() {
                Some("first-push") => {
                    assert_eq!(r.state()["block"]["socket"], 1);
                    assert_eq!(r.state()["block"]["moves"], 1);
                }
                Some("second-push") => {
                    assert_eq!(r.state()["block"]["socket"], 2);
                    assert_eq!(r.state()["block"]["moves"], 2);
                }
                Some("block-support") => {
                    assert_eq!(r.character("jumper")["y"], 750, "{label}: {}", r.state());
                    assert_eq!(r.character("jumper")["grounded"], true);
                }
                Some("plate-a") => {
                    assert_eq!(
                        r.puzzle()["plates"][0]["occupants"],
                        json!(["jumper"]),
                        "{label}: {}",
                        r.state()
                    );
                }
                Some("plate-b") => {
                    assert_eq!(r.puzzle()["plates"][0]["pressed"], true);
                    assert_eq!(
                        r.puzzle()["plates"][1]["occupants"],
                        json!(["strong"]),
                        "{label}: {}",
                        r.state()
                    );
                }
                Some("exchange") => {
                    assert_eq!(r.puzzle()["plates"][0]["pressed"], false);
                    assert_eq!(r.puzzle()["door"]["state"], "open_plate");
                }
                Some("jumper-exit") => {
                    assert_eq!(
                        r.puzzle()["exit"],
                        json!({"jumper":true,"strong":false}),
                        "{label}: {}",
                        r.state()
                    );
                    assert_eq!(r.puzzle()["complete"], false);
                }
                Some("complete") => {
                    assert_eq!(r.puzzle()["complete"], true, "{label}: {}", r.state());
                }
                _ => {}
            }
        }
        let recording = serde_json::to_value(game::recording(&r.app).unwrap()).unwrap();
        let decoded: game::Recording = serde_json::from_value(recording.clone()).unwrap();
        let mut playback = Run::new();
        for (index, frame) in decoded.frames.iter().enumerate() {
            playback
                .app
                .world_mut()
                .insert_resource(frame.decode(&game::SCHEMA).unwrap());
            playback.app.advance_fixed(1);
            let actual = game::status(&playback.app);
            assert_eq!(
                actual,
                r.trace[index + 1],
                "{label} recording tick {}",
                index + 1
            );
            playback.trace.push(actual);
        }
        scenarios.insert(format!("{label}-recording-every-tick"), playback.trace);
        let expected = r.state().clone();
        game::replay(
            &mut r.app,
            serde_json::from_value(recording.clone()).unwrap(),
        )
        .unwrap();
        let actual = game::status(&r.app);
        for key in [
            "room",
            "characters",
            "puzzle",
            "block",
            "active_character",
            "session_tick",
            "consumed_input",
        ] {
            assert_eq!(actual[key], expected[key], "{label} replay {key}");
        }
        r.trace.push(actual);
        r.tick(
            &[Action::Interact, Action::Jump, Action::Left, Action::Switch],
            8,
        );
        for key in [
            "characters",
            "puzzle",
            "block",
            "active_character",
            "session_tick",
        ] {
            assert_eq!(r.state()[key], expected[key], "completion freeze {key}");
        }
        r.tick(
            &[
                Action::Restart,
                Action::Jump,
                Action::Right,
                Action::Interact,
            ],
            1,
        );
        fresh(&r);
        r.tick(&[Action::Jump, Action::Right, Action::Interact], 3);
        assert_eq!(r.character("jumper")["x"], 1500);
        assert_eq!(r.character("jumper")["y"], 0);
        recordings.insert(label, recording);
        scenarios.insert(format!("{label}-ordinary-solution-replay-reset"), r.trace);
    }
    // Check rejection precedence in the real fixed schedule. Rejected pushes may
    // still move/jump the character; they must never move the block.
    for (label, strong, grounded, x, z, request, reason) in [
        (
            "wrong-character-priority",
            false,
            false,
            1500,
            6500,
            vec![Action::Interact, Action::Jump],
            "wrong_character",
        ),
        (
            "airborne-priority",
            true,
            false,
            1500,
            6500,
            vec![Action::Interact],
            "not_grounded",
        ),
        (
            "jump-push-priority",
            true,
            true,
            5500,
            6500,
            vec![Action::Interact, Action::Jump, Action::Up],
            "not_grounded",
        ),
        (
            "missing-direction-priority",
            true,
            true,
            3500,
            6500,
            vec![Action::Interact],
            "invalid_direction",
        ),
        (
            "multiple-directions",
            true,
            true,
            5500,
            6500,
            vec![Action::Interact, Action::Up, Action::Right],
            "invalid_direction",
        ),
        (
            "opposing-directions",
            true,
            true,
            5500,
            6500,
            vec![Action::Interact, Action::Up, Action::Down],
            "invalid_direction",
        ),
        (
            "off-rail-direction",
            true,
            true,
            4500,
            5500,
            vec![Action::Interact, Action::Right],
            "invalid_direction",
        ),
        (
            "stance-priority",
            true,
            true,
            3500,
            6500,
            vec![Action::Interact, Action::Up],
            "invalid_stance",
        ),
        (
            "initial-rail-end",
            true,
            true,
            5500,
            4500,
            vec![Action::Interact, Action::Down],
            "rail_end",
        ),
    ] {
        let mut r = Run::new();
        if strong {
            r.strong();
        }
        r.place(
            if strong { 1 } else { 0 },
            x,
            if grounded { 0 } else { 500 },
            z,
            grounded,
        );
        r.tick(&request, 1);
        assert_eq!(
            r.state()["block"]["last_rejection"],
            reason,
            "{label}: {}",
            r.state()
        );
        assert_eq!(r.state()["block"]["socket"], 0);
        if label == "jump-push-priority" {
            assert_eq!(r.character("strong")["y"], 90);
        }
        scenarios.insert(label.into(), r.trace);
    }
    for (label, x, y, z, settle, reason) in [
        (
            "supported-inactive-character",
            5500,
            750,
            5500,
            true,
            Some("block_occupied"),
        ),
        (
            "sweep-middle-body",
            5500,
            0,
            5000,
            false,
            Some("path_obstructed"),
        ),
        (
            "destination-body",
            5500,
            0,
            4500,
            false,
            Some("path_obstructed"),
        ),
        (
            "airborne-below-block-top",
            5500,
            740,
            4500,
            false,
            Some("path_obstructed"),
        ),
        (
            "one-mm-positive-overlap",
            6149,
            0,
            4500,
            false,
            Some("path_obstructed"),
        ),
        ("exact-face-contact-clear", 6150, 0, 4500, false, None),
        (
            "airborne-entirely-above-clear",
            5500,
            750,
            4500,
            false,
            None,
        ),
    ] {
        let mut r = Run::new();
        r.strong();
        r.place(1, 5500, 0, 6500, true);
        r.place(0, x, y, z, settle);
        if settle {
            r.tick(&[], 1);
        }
        r.tick(&[Action::Interact, Action::Up], 1);
        assert_eq!(
            r.state()["block"]["last_rejection"],
            json!(reason),
            "{label}: {}",
            r.state()
        );
        assert_eq!(
            r.state()["block"]["socket"],
            if reason.is_none() { 1 } else { 0 }
        );
        if reason.is_none() {
            assert_eq!(
                r.character("strong")["z"],
                6500,
                "accepted operation consumes movement"
            );
        }
        scenarios.insert(label.into(), r.trace);
    }
    // Ordinary inputs cover both key orders, a distant approach, contact, and
    // release/repress after success. These traces run identically in native/WASM.
    for (label, first, z) in [
        ("direction-before-e-at-contact", vec![Action::Up], 6150),
        (
            "e-before-direction-at-contact",
            vec![Action::Interact],
            6150,
        ),
        ("held-combination-during-approach", vec![Action::Up], 7500),
    ] {
        let mut r = Run::new();
        r.strong();
        r.place(1, 5740, 0, z, true);
        r.tick(&first, 20);
        r.tick(&[Action::Interact, Action::Up], 60);
        assert_eq!(r.state()["block"]["moves"], 1, "{label}");
        assert_eq!(r.character("strong")["z"], 5150, "{label}");
        r.tick(&[Action::Interact], 1);
        r.tick(&[Action::Interact, Action::Up], 1);
        assert_eq!(
            r.state()["block"]["moves"],
            1,
            "direction alone cannot rearm"
        );
        r.tick(&[Action::Up], 1);
        r.tick(&[Action::Interact, Action::Up], 1);
        assert_eq!(r.state()["block"]["moves"], 2, "{label}");
        scenarios.insert(label.into(), r.trace);
    }
    for (label, y) in [("occupied-hold-retry", 750), ("obstructed-hold-retry", 0)] {
        let mut r = Run::new();
        r.strong();
        r.place(1, 5500, 0, 6150, true);
        r.place(0, 5500, y, if y == 750 { 5500 } else { 4500 }, true);
        r.tick(&[], 1); // Resolve block support before requesting a push.
        r.tick(&[Action::Interact, Action::Up], 3);
        assert_eq!(r.state()["block"]["moves"], 0);
        assert_eq!(
            r.state()["block"]["last_rejection"],
            if y == 750 {
                "block_occupied"
            } else {
                "path_obstructed"
            }
        );
        r.place(0, 1500, 0, 6500, true);
        r.tick(&[Action::Interact, Action::Up], 1);
        assert_eq!(r.state()["block"]["moves"], 1);
        scenarios.insert(label.into(), r.trace);
    }
    let mut edges = Run::new();
    edges.strong();
    edges.place(1, 5500, 0, 6500, true);
    edges.tick(&[Action::Interact, Action::Up], 1);
    edges.tick(&[Action::Interact, Action::Up], 16);
    assert_eq!(edges.state()["block"]["moves"], 1, "held E cannot chain");
    edges.tick(&[], 1);
    edges.tick(&[Action::Interact, Action::Up], 1);
    assert_eq!(edges.state()["block"]["socket"], 2);
    assert_eq!(edges.character("strong")["z"], 5540);
    edges.tick(&[Action::Switch, Action::Interact, Action::Up], 1);
    edges.tick(&[Action::Interact, Action::Up], 3);
    assert_eq!(edges.state()["block"]["moves"], 2);
    assert_eq!(edges.character("jumper")["z"], 6500);
    assert_eq!(
        edges.state()["block"]["last_rejection"],
        Value::Null,
        "held push remains suppressed after switch"
    );
    scenarios.insert(
        "press-edge-atomic-move-switch-suppression".into(),
        edges.trace,
    );
    let mut reverse = Run::new();
    reverse.strong();
    reverse.place(1, 5500, 0, 6500, true);
    reverse.tick(&[Action::Interact, Action::Up], 1);
    reverse.tick(&[Action::Right], 17);
    reverse.tick(&[Action::Up], 50);
    reverse.tick(&[Action::Left], 17);
    reverse.tick(&[Action::Interact, Action::Down], 1);
    assert_eq!(
        reverse.state()["block"]["socket"],
        0,
        "reachable intermediate reverse stance"
    );
    assert_eq!(reverse.state()["block"]["moves"], 2);
    scenarios.insert(
        "intermediate-reverse-via-east-floor-route".into(),
        reverse.trace,
    );
    // Strong cannot mount even the lower block; Jumper cannot jump 2m from floor.
    for (label, index, x, z) in [
        ("strong-cannot-mount-block", 1, 5500, 6200),
        ("jumper-cannot-reach-ledge-from-floor", 0, 5500, 3300),
    ] {
        let mut r = Run::new();
        if index == 1 {
            r.strong();
        }
        r.place(index, x, 0, z, true);
        r.tick(&[Action::Jump, Action::Up], 40);
        assert_eq!(
            r.character(if index == 0 { "jumper" } else { "strong" })["y"],
            0
        );
        assert_eq!(r.puzzle()["plates"][0]["pressed"], false);
        scenarios.insert(label.into(), r.trace);
    }
    // Most generous physically supported initial launch. One millimetre of overlap
    // avoids mistaking exact edge contact for support. Full northward air control
    // still cannot clear the ledge at the descending height-crossing tick.
    let mut initial = Run::new();
    initial.place(0, 5500, 750, 4851, true);
    initial.tick(&[], 1);
    assert_eq!(initial.character("jumper")["grounded"], true);
    initial.tick(&[Action::Jump, Action::Up], 40);
    assert_eq!(initial.character("jumper")["y"], 0);
    assert_eq!(initial.puzzle()["plates"][0]["pressed"], false);
    scenarios.insert(
        "initial-socket-best-edge-launch-cannot-bypass".into(),
        initial.trace,
    );
    for fallen in [0, 1] {
        let mut r = Run::new();
        r.strong();
        r.place(1, 5500, 0, 6500, true);
        r.tick(&[Action::Interact, Action::Up], 1);
        r.place(fallen, 500, -2001, 6500, false);
        r.tick(&[Action::Right, Action::Jump], 1);
        fresh(&r);
        assert_eq!(r.state()["recovery_message_ticks"], 120);
        r.tick(&[Action::Right, Action::Jump], 4);
        assert_eq!(r.character("jumper")["x"], 1500);
        scenarios.insert(format!("recovery-{fallen}-restores-current-room"), r.trace);
    }
    json!({"format_version":1,"recordings":recordings,"scenarios":scenarios})
}
#[cfg(test)]
mod tests {
    #[test]
    fn room_two_acceptance() {
        super::run();
    }
}
