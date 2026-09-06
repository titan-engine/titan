//! Native/actual-WASM acceptance using production movement with isolated geometry.
//! Fixtures are compiled only with `movement-acceptance`, never player commands.
use crate::game::{
    self, Action, Position,
    movement::{self, Movement, Solid},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use titan::{
    App, Startup,
    input::{ActionValue, InputTracker},
};

fn position(x: i32, y: i32, z: i32) -> Position {
    Position { x, y, z }
}
fn solid(name: &'static str, min: (i32, i32, i32), max: (i32, i32, i32)) -> Solid {
    Solid {
        name,
        min: position(min.0, min.1, min.2),
        max: position(max.0, max.1, max.2),
    }
}
fn floor() -> Solid {
    solid("floor", (-20000, -1000, -20000), (20000, 0, 20000))
}
fn sample(p: Position, m: Movement) -> Value {
    json!({"position":p,"movement":m})
}
fn advance(
    trace: &mut Vec<Value>,
    p: &mut Position,
    m: &mut Movement,
    delta: (i32, i32),
    jump: bool,
    speed: i32,
    solids: &[Solid],
) {
    movement::advance(p, m, delta.0, delta.1, jump, speed, solids);
    trace.push(sample(*p, *m));
}
fn new_app() -> App {
    let mut app = game::build_game();
    app.update_schedule(Startup);
    app
}
fn tick(
    app: &mut App,
    tracker: &mut InputTracker<Action>,
    actions: &[Action],
    trace: &mut Vec<Value>,
) {
    app.world_mut()
        .insert_resource(tracker.sample(actions.iter().map(|a| (*a, ActionValue::PRESSED))));
    app.advance_fixed(1);
    trace.push(game::status(app));
}
fn character<'a>(state: &'a Value, name: &str) -> &'a Value {
    &state["characters"][name]
}

fn inspected_request(
    app: &mut App,
    inspector: &mut titan::inspection::Inspector,
    request: titan_protocol::Request,
) {
    let response = inspector.handle(
        app,
        &titan_protocol::RequestEnvelope::new("movement-acceptance", request),
    );
    assert!(
        matches!(
            response.outcome,
            titan_protocol::ResponseOutcome::Success { .. }
        ),
        "{response:?}"
    );
}
fn inject_at(
    app: &mut App,
    inspector: &mut titan::inspection::Inspector,
    frame: u64,
    actions: &[&str],
) {
    inspected_request(
        app,
        inspector,
        titan_protocol::Request::InjectInput {
            frame,
            actions: actions
                .iter()
                .map(|name| ((*name).into(), titan_protocol::InputValue::Button(true)))
                .collect(),
        },
    );
}
fn injected_tick(
    app: &mut App,
    inspector: &mut titan::inspection::Inspector,
    actions: &[&str],
    trace: &mut Vec<Value>,
) {
    let frame = game::status(app)["frame"].as_u64().unwrap() + 1;
    inject_at(app, inspector, frame, actions);
    inspected_request(app, inspector, titan_protocol::Request::Step { frames: 1 });
    trace.push(game::status(app));
}
fn assert_inspected_replay(
    app: &mut App,
    inspector: &mut titan::inspection::Inspector,
    trace: &mut Vec<Value>,
) {
    let expected = game::status(app);
    let recording = serde_json::to_value(game::recording(app).unwrap()).unwrap();
    inspected_request(
        app,
        inspector,
        titan_protocol::Request::Invoke {
            name: "replay".into(),
            arguments: BTreeMap::from([("recording".into(), recording)]),
        },
    );
    let replayed = game::status(app);
    for key in [
        "characters",
        "active_character",
        "consumed_input",
        "session_tick",
        "blocked_actions",
        "recovery_message_ticks",
    ] {
        assert_eq!(replayed[key], expected[key], "inspector replay {key}");
    }
    trace.push(replayed);
}

/// All assertions execute independently in both targets; JSON carries every tick.
pub fn run() -> Value {
    let mut scenarios = BTreeMap::<String, Vec<Value>>::new();
    for (name, speed, apex) in [("jumper", 180, 1530), ("strong", 100, 450)] {
        let mut app = new_app();
        let mut tracker = InputTracker::default();
        let mut trace = vec![game::status(&app)];
        if name == "strong" {
            tick(&mut app, &mut tracker, &[Action::Switch], &mut trace);
            tick(&mut app, &mut tracker, &[], &mut trace);
        }
        // Holding Space through landing must never repeat the jump.
        for _ in 0..65 {
            tick(&mut app, &mut tracker, &[Action::Jump], &mut trace);
        }
        assert_eq!(
            trace
                .iter()
                .map(|s| character(s, name)["y"].as_i64().unwrap())
                .max(),
            Some(apex)
        );
        assert_eq!(character(trace.last().unwrap(), name)["y"], 0);
        assert_eq!(character(trace.last().unwrap(), name)["grounded"], true);
        tick(&mut app, &mut tracker, &[], &mut trace);
        tick(&mut app, &mut tracker, &[Action::Jump], &mut trace);
        assert_eq!(character(trace.last().unwrap(), name)["y"], speed - 10);
        scenarios.insert(format!("{name}-apex-held-jump-and-repress"), trace);

        for height in [750, 1000, 2000] {
            let solids = [
                floor(),
                solid("gate", (4000, 0, 1000), (7000, height, 3000)),
            ];
            let mut p = position(5500, 0, 3700);
            let mut m = Movement::default();
            let mut trace = vec![sample(p, m)];
            for frame in 0..65 {
                advance(
                    &mut trace,
                    &mut p,
                    &mut m,
                    (0, if frame < 20 { -60 } else { 0 }),
                    frame == 0,
                    speed,
                    &solids,
                );
            }
            let expected = if name == "jumper" && height < 2000 {
                height
            } else {
                0
            };
            assert_eq!(p.y, expected, "{name} gate {height}");
            assert!(m.grounded);
            assert_eq!(
                m.support,
                Some(if expected == 0 { "floor" } else { "gate" })
            );
            if expected == 0 {
                assert_eq!(p.z, 3200);
            }
            scenarios.insert(format!("{name}-height-gate-{height}"), trace);
        }
    }

    // Actual room-2 dimensions, with no socket-aware gameplay conditions.
    for (socket, launch) in [(5500, 4851), (4500, 3851), (3500, 3500)] {
        for (name, speed) in [("jumper", 180), ("strong", 100)] {
            let solids = [
                floor(),
                solid("ledge", (4000, 0, 1000), (7000, 2000, 3000)),
                solid("block", (5050, 0, socket - 450), (5950, 750, socket + 450)),
            ];
            let mut p = position(5500, 750, launch);
            let mut m = Movement::default();
            let mut trace = vec![sample(p, m)];
            for frame in 0..80 {
                advance(
                    &mut trace,
                    &mut p,
                    &mut m,
                    (0, if frame < 40 { -60 } else { 0 }),
                    frame == 0,
                    speed,
                    &solids,
                );
            }
            let reached = name == "jumper" && socket != 5500;
            assert_eq!(p.y == 2000, reached, "{name} socket {socket}");
            assert!(m.grounded);
            scenarios.insert(format!("{name}-block-socket-{socket}"), trace);
        }
    }
    {
        let solids = [floor(), solid("step", (4000, 0, 1000), (7000, 750, 3000))];
        let mut p = position(5500, 0, 3700);
        let mut m = Movement::default();
        let mut trace = vec![sample(p, m)];
        for _ in 0..40 {
            advance(&mut trace, &mut p, &mut m, (0, -60), false, 180, &solids);
        }
        assert_eq!(p, position(5500, 0, 3200));
        assert_eq!(m.collisions.z, Some("step"));
        scenarios.insert("no-walking-step-up".into(), trace);
    }
    for (name, z, jump, expected_y) in [
        ("positive-overlap", 3199, false, 1000),
        ("edge-only", 3200, false, 990),
        ("edge-no-coyote-jump", 3200, true, 990),
    ] {
        let solids = [floor(), solid("ledge", (4000, 0, 1000), (7000, 1000, 3000))];
        let mut p = position(5500, 1000, z);
        let mut m = Movement::default();
        let mut trace = vec![sample(p, m)];
        advance(&mut trace, &mut p, &mut m, (0, 0), jump, 180, &solids);
        assert_eq!(p.y, expected_y);
        assert_eq!(m.grounded, z == 3199);
        scenarios.insert(name.into(), trace);
    }
    {
        let solids = [floor(), solid("ledge", (4000, 0, 1000), (7000, 1000, 3000))];
        let mut p = position(5500, 1000, 3199);
        let mut m = Movement::default();
        let mut trace = vec![sample(p, m)];
        advance(&mut trace, &mut p, &mut m, (0, 60), false, 180, &solids);
        assert_eq!(p.y, 990, "walking off starts gravity in the same tick");
        for _ in 0..30 {
            advance(&mut trace, &mut p, &mut m, (0, 0), false, 180, &solids);
        }
        assert_eq!(p.y, 0);
        assert_eq!(m.support, Some("floor"));
        scenarios.insert("walk-off-safe-floor-landing".into(), trace);
    }
    // Reverse solid ordering verifies nearest crossed faces rather than list order.
    for reversed in [false, true] {
        let mut solids = vec![
            floor(),
            solid("low", (4000, 1000, 1000), (7000, 1200, 3000)),
            solid("high", (4000, 3000, 1000), (7000, 3200, 3000)),
        ];
        if reversed {
            solids.reverse();
        }
        let mut p = position(5500, 6000, 2000);
        let mut m = Movement {
            velocity_y: -10000,
            grounded: false,
            ..Movement::default()
        };
        let mut trace = vec![sample(p, m)];
        advance(&mut trace, &mut p, &mut m, (0, 0), false, 180, &solids);
        assert_eq!(p.y, 3200);
        assert_eq!(m.support, Some("high"));
        p = position(5500, 0, 2000);
        m = Movement {
            velocity_y: 10000,
            grounded: false,
            ..Movement::default()
        };
        trace.push(sample(p, m));
        advance(&mut trace, &mut p, &mut m, (0, 0), false, 180, &solids);
        assert_eq!(p.y, 100);
        assert_eq!(m.collisions.ceiling, Some("low"));
        assert_eq!(m.velocity_y, 0);
        advance(&mut trace, &mut p, &mut m, (0, 0), false, 180, &solids);
        assert_eq!(p.y, 90);
        scenarios.insert(format!("vertical-sweep-nearest-{reversed}"), trace);
    }
    for (name, start, delta, expected) in
        [("right", 1000, 20000, 3800), ("left", 10000, -20000, 8200)]
    {
        let solids = [
            floor(),
            solid("near", (4000, 0, 1000), (5000, 4000, 3000)),
            solid("far", (7000, 0, 1000), (8000, 4000, 3000)),
        ];
        let mut p = position(start, 0, 2000);
        let mut m = Movement::default();
        let mut trace = vec![sample(p, m)];
        advance(&mut trace, &mut p, &mut m, (delta, 0), false, 180, &solids);
        assert_eq!(p.x, expected);
        assert!(m.collisions.x.is_some());
        scenarios.insert(format!("horizontal-high-speed-{name}"), trace);
    }

    {
        let mut app = new_app();
        let mut tracker = InputTracker::default();
        let mut trace = vec![game::status(&app)];
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right],
            &mut trace,
        );
        let first = trace.last().unwrap().clone();
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right, Action::Switch],
            &mut trace,
        );
        for _ in 0..5 {
            tick(
                &mut app,
                &mut tracker,
                &[Action::Jump, Action::Right, Action::Switch],
                &mut trace,
            );
        }
        let switched = trace.last().unwrap();
        assert_eq!(switched["active_character"], "strong");
        assert_eq!(
            character(switched, "jumper")["x"],
            character(&first, "jumper")["x"]
        );
        assert!(character(switched, "jumper")["y"].as_i64().unwrap() > 170);
        assert_eq!(character(switched, "strong")["y"], 0);
        assert_eq!(character(switched, "strong")["x"], 3500);
        tick(&mut app, &mut tracker, &[], &mut trace);
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right],
            &mut trace,
        );
        assert_eq!(character(trace.last().unwrap(), "strong")["y"], 90);
        assert_eq!(character(trace.last().unwrap(), "strong")["x"], 3560);
        for _ in 0..60 {
            tick(&mut app, &mut tracker, &[], &mut trace);
        }
        for name in ["jumper", "strong"] {
            assert_eq!(character(trace.last().unwrap(), name)["grounded"], true);
        }
        let expected = game::status(&app);
        let recording = game::recording(&app).unwrap();
        game::replay(&mut app, recording).unwrap();
        let replayed = game::status(&app);
        for key in [
            "characters",
            "active_character",
            "consumed_input",
            "session_tick",
        ] {
            assert_eq!(replayed[key], expected[key], "replay {key}");
        }
        trace.push(replayed);
        scenarios.insert(
            "midair-switch-held-gating-inactive-gravity-replay".into(),
            trace,
        );
    }
    {
        let mut app = new_app();
        let mut tracker = InputTracker::default();
        let mut trace = vec![game::status(&app)];
        tick(&mut app, &mut tracker, &[Action::Jump], &mut trace);
        tick(&mut app, &mut tracker, &[], &mut trace);
        tick(&mut app, &mut tracker, &[Action::Jump], &mut trace);
        assert_eq!(
            character(trace.last().unwrap(), "jumper")["velocity_y"],
            150
        );
        // An attempted new airborne press cannot become a buffered landing jump.
        for _ in 0..60 {
            tick(&mut app, &mut tracker, &[Action::Jump], &mut trace);
        }
        assert_eq!(character(trace.last().unwrap(), "jumper")["y"], 0);
        assert_eq!(trace.last().unwrap()["session_generation"], 0);
        scenarios.insert("no-double-jump-or-landing-buffer".into(), trace);
    }
    {
        let mut app = new_app();
        game::fixture_set_character(&mut app, 1, position(1800, 0, 6500), 0, true);
        let mut tracker = InputTracker::default();
        let mut trace = vec![game::status(&app)];
        for _ in 0..10 {
            tick(&mut app, &mut tracker, &[Action::Right], &mut trace);
        }
        assert_eq!(character(trace.last().unwrap(), "jumper")["x"], 2100);
        assert_eq!(character(trace.last().unwrap(), "strong")["x"], 1800);
        game::fixture_set_character(&mut app, 0, position(1800, 1000, 6500), -100, false);
        for _ in 0..30 {
            tick(&mut app, &mut tracker, &[], &mut trace);
        }
        assert_eq!(character(trace.last().unwrap(), "jumper")["y"], 0);
        assert_eq!(
            character(trace.last().unwrap(), "jumper")["support"],
            "floor"
        );
        scenarios.insert("characters-pass-through-and-never-support".into(), trace);
    }
    {
        let mut app = new_app();
        let mut tracker = InputTracker::default();
        let mut trace = vec![game::status(&app)];
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right],
            &mut trace,
        );
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right, Action::Restart],
            &mut trace,
        );
        let reset = trace.last().unwrap();
        assert_eq!(reset["session_generation"], 1);
        assert_eq!(reset["session_tick"], 0);
        assert_eq!(reset["recovery_message_ticks"], 0);
        for _ in 0..5 {
            tick(
                &mut app,
                &mut tracker,
                &[Action::Jump, Action::Right],
                &mut trace,
            );
        }
        assert_eq!(character(trace.last().unwrap(), "jumper")["x"], 1500);
        assert_eq!(character(trace.last().unwrap(), "jumper")["y"], 0);
        tick(&mut app, &mut tracker, &[], &mut trace);
        tick(&mut app, &mut tracker, &[Action::Jump], &mut trace);
        assert_eq!(character(trace.last().unwrap(), "jumper")["y"], 170);
        scenarios.insert("restart-midair-clears-motion-and-held-input".into(), trace);
    }
    for fallen in [0, 1] {
        let mut app = new_app();
        let mut tracker = InputTracker::default();
        let mut trace = vec![game::status(&app)];
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right],
            &mut trace,
        );
        game::fixture_set_character(&mut app, fallen, position(500, -2000, 6500), -10, false);
        trace.push(game::status(&app));
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right],
            &mut trace,
        );
        let reset = trace.last().unwrap().clone();
        assert_eq!(reset["session_generation"], 1);
        assert_eq!(reset["session_tick"], 0);
        assert_eq!(reset["active_character"], "jumper");
        assert_eq!(reset["recorded_ticks"], 0);
        assert_eq!(reset["pending_inputs"], 0);
        assert_eq!(reset["recovery_message_ticks"], 120);
        for (name, x) in [("jumper", 1500), ("strong", 3500)] {
            let c = character(&reset, name);
            assert_eq!(c["x"], x);
            assert_eq!(c["y"], 0);
            assert_eq!(c["z"], 6500);
            assert_eq!(c["velocity_y"], 0);
            assert_eq!(c["grounded"], true);
        }
        tick(
            &mut app,
            &mut tracker,
            &[Action::Jump, Action::Right],
            &mut trace,
        );
        assert_eq!(character(trace.last().unwrap(), "jumper")["x"], 1500);
        assert_eq!(character(trace.last().unwrap(), "jumper")["y"], 0);
        assert!(
            trace.last().unwrap()["frame"].as_u64().unwrap() > reset["frame"].as_u64().unwrap()
        );
        scenarios.insert(
            format!("below-floor-reconstructs-both-character-{fallen}"),
            trace,
        );
    }
    // Complete injected snapshots have their own source tracker. Reconstruction
    // must not turn a held action into a new press when that source resumes.
    for held in ["restart", "jump", "switch"] {
        let mut app = new_app();
        let mut inspector = game::configured_inspector(
            titan::inspection::InspectionConfig::controlled("movement-acceptance", "headless"),
        );
        let mut trace = vec![game::status(&app)];
        injected_tick(&mut app, &mut inspector, &["restart", held], &mut trace);
        for _ in 0..3 {
            injected_tick(&mut app, &mut inspector, &[held], &mut trace);
        }
        let held_state = trace.last().unwrap();
        assert_eq!(
            held_state["session_generation"], 1,
            "held {held} must not repeat reconstruction"
        );
        assert_eq!(held_state["active_character"], "jumper");
        assert_eq!(character(held_state, "jumper")["y"], 0);
        // Omitting an action from a complete snapshot is a real release.
        injected_tick(&mut app, &mut inspector, &[], &mut trace);
        injected_tick(&mut app, &mut inspector, &[held], &mut trace);
        let repressed = trace.last().unwrap();
        match held {
            "restart" => assert_eq!(repressed["session_generation"], 2),
            "jump" => assert_eq!(character(repressed, "jumper")["y"], 170),
            "switch" => assert_eq!(repressed["active_character"], "strong"),
            _ => unreachable!(),
        }
        assert_inspected_replay(&mut app, &mut inspector, &mut trace);
        scenarios.insert(
            format!("injected-restart-held-{held}-release-repress-replay"),
            trace,
        );
    }
    for fallen in [0, 1] {
        let mut app = new_app();
        let mut inspector = game::configured_inspector(
            titan::inspection::InspectionConfig::controlled("movement-acceptance", "headless"),
        );
        let mut trace = vec![game::status(&app)];
        injected_tick(
            &mut app,
            &mut inspector,
            &["jump", "switch", "right"],
            &mut trace,
        );
        game::fixture_set_character(&mut app, fallen, position(500, -2000, 6500), -10, false);
        let future = game::status(&app)["frame"].as_u64().unwrap() + 5;
        inject_at(&mut app, &mut inspector, future, &["restart"]);
        assert_eq!(game::status(&app)["pending_inputs"], 1);
        trace.push(game::status(&app));
        injected_tick(
            &mut app,
            &mut inspector,
            &["jump", "switch", "right"],
            &mut trace,
        );
        let reset = trace.last().unwrap();
        assert_eq!(reset["session_generation"], 1);
        assert_eq!(reset["session_tick"], 0);
        assert_eq!(reset["recovery_message_ticks"], 120);
        assert_eq!(reset["pending_inputs"], 0);
        for _ in 0..5 {
            injected_tick(
                &mut app,
                &mut inspector,
                &["jump", "switch", "right"],
                &mut trace,
            );
        }
        let held = trace.last().unwrap();
        assert_eq!(held["session_generation"], 1);
        assert_eq!(held["active_character"], "jumper");
        assert_eq!(character(held, "jumper")["x"], 1500);
        assert_eq!(character(held, "jumper")["y"], 0);
        assert_inspected_replay(&mut app, &mut inspector, &mut trace);
        injected_tick(&mut app, &mut inspector, &[], &mut trace);
        injected_tick(&mut app, &mut inspector, &["jump", "right"], &mut trace);
        assert_eq!(character(trace.last().unwrap(), "jumper")["y"], 170);
        assert_eq!(character(trace.last().unwrap(), "jumper")["x"], 1560);
        injected_tick(&mut app, &mut inspector, &[], &mut trace);
        injected_tick(&mut app, &mut inspector, &["switch"], &mut trace);
        assert_eq!(trace.last().unwrap()["active_character"], "strong");
        assert_inspected_replay(&mut app, &mut inspector, &mut trace);
        scenarios.insert(
            format!("injected-fall-{fallen}-held-input-release-repress-replay"),
            trace,
        );
    }
    json!({"format_version":1,"scenarios":scenarios})
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_movement_scenarios() {
        super::run();
    }
}
