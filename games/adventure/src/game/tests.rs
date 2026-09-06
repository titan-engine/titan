use super::*;
fn app() -> App {
    let mut app = build_game();
    app.update_schedule(Startup);
    app.world_mut().resource_mut::<Session>().unwrap().phase = Phase::Playing;
    app.world_mut()
        .resource_mut::<Session>()
        .unwrap()
        .origin
        .phase = Phase::Playing;
    app
}
fn tick_with(app: &mut App, tracker: &mut InputTracker<Action>, actions: &[Action]) {
    app.world_mut()
        .insert_resource(tracker.sample(actions.iter().map(|a| (*a, ActionValue::PRESSED))));
    app.advance_fixed(1);
}
fn pos(app: &App, name: &str) -> Position {
    let s = status(app);
    Position {
        x: s["characters"][name]["x"].as_i64().unwrap() as i32,
        y: s["characters"][name]["y"].as_i64().unwrap() as i32,
        z: s["characters"][name]["z"].as_i64().unwrap() as i32,
    }
}
#[test]
fn switching_blocks_held_actions_until_release_and_accepts_fresh_actions() {
    let mut a = app();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Right]);
    assert_eq!(pos(&a, "jumper").x, 1560);
    tick_with(&mut a, &mut t, &[Action::Right, Action::Switch]);
    assert_eq!(status(&a)["active_character"], "strong");
    assert_eq!(pos(&a, "strong"), initial_position(1));
    tick_with(&mut a, &mut t, &[Action::Right, Action::Switch, Action::Up]);
    assert_eq!(status(&a)["active_character"], "strong");
    assert_eq!(
        pos(&a, "strong"),
        Position {
            x: 3500,
            y: 0,
            z: 6440
        }
    );
    tick_with(&mut a, &mut t, &[]);
    tick_with(&mut a, &mut t, &[Action::Right]);
    assert_eq!(pos(&a, "strong").x, 3560);
    assert_eq!(pos(&a, "jumper").x, 1560);
    tick_with(&mut a, &mut t, &[Action::Switch]);
    assert_eq!(status(&a)["active_character"], "jumper");
}
#[test]
fn diagonal_cancelled_axes_and_bounds_are_exact() {
    let mut a = app();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Up, Action::Right]);
    assert_eq!(
        pos(&a, "jumper"),
        Position {
            x: 1542,
            y: 0,
            z: 6458
        }
    );
    tick_with(&mut a, &mut t, &[Action::Left, Action::Right]);
    assert_eq!(
        pos(&a, "jumper"),
        Position {
            x: 1542,
            y: 0,
            z: 6458
        }
    );
    for _ in 0..250 {
        tick_with(&mut a, &mut t, &[Action::Up, Action::Left]);
    }
    assert_eq!(
        pos(&a, "jumper"),
        Position {
            x: 200,
            y: 0,
            z: 200
        }
    );
    for _ in 0..400 {
        tick_with(&mut a, &mut t, &[Action::Down, Action::Right]);
    }
    assert_eq!(
        pos(&a, "jumper"),
        Position {
            x: 6800,
            y: 0,
            z: 7800
        }
    );
    assert_eq!(pos(&a, "strong"), initial_position(1));
}
#[test]
fn restart_has_precedence_and_does_not_repeat_while_held() {
    let mut a = app();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Right]);
    tick_with(
        &mut a,
        &mut t,
        &[Action::Restart, Action::Switch, Action::Right],
    );
    assert_eq!(status(&a)["active_character"], "jumper");
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    let generation = status(&a)["session_generation"].clone();
    tick_with(
        &mut a,
        &mut t,
        &[Action::Restart, Action::Switch, Action::Right],
    );
    assert_eq!(status(&a)["session_generation"], generation);
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    tick_with(&mut a, &mut t, &[]);
    tick_with(&mut a, &mut t, &[Action::Right]);
    assert_eq!(pos(&a, "jumper").x, 1560);
    let recorded = recording(&a).unwrap();
    let before = status(&a);
    replay(&mut a, recorded).unwrap();
    let after = status(&a);
    for key in [
        "characters",
        "active_character",
        "session_tick",
        "consumed_input",
        "blocked_actions",
    ] {
        assert_eq!(before[key], after[key], "{key}");
    }
}
#[test]
fn replay_preserves_switch_suppression_and_rejects_invalid_before_mutation() {
    let mut a = app();
    let mut t = InputTracker::default();
    for actions in [
        vec![Action::Right],
        vec![Action::Right, Action::Switch],
        vec![Action::Right, Action::Up],
        vec![],
        vec![Action::Left],
    ] {
        tick_with(&mut a, &mut t, &actions);
    }
    let before = status(&a);
    let r = recording(&a).unwrap();
    replay(&mut a, r).unwrap();
    let after = status(&a);
    for key in [
        "characters",
        "active_character",
        "session_tick",
        "consumed_input",
        "blocked_actions",
    ] {
        assert_eq!(before[key], after[key], "{key}");
    }
    let mut r = recording(&a).unwrap();
    r.fixture = "wrong".into();
    let before = status(&a);
    assert!(replay(&mut a, r).is_err());
    assert_eq!(status(&a), before);
    let mut r = recording(&a).unwrap();
    r.truncated = true;
    assert!(replay(&mut a, r).is_err());
    assert_eq!(status(&a), before);
}
#[test]
fn recording_is_bounded_and_restart_restores_a_fresh_origin() {
    let mut a = app();
    a.advance_fixed(MAX_RECORDING_TICKS as u64 + 1);
    assert_eq!(recording(&a).unwrap().frames.len(), MAX_RECORDING_TICKS);
    assert!(recording(&a).unwrap().truncated);
    restart(&mut a);
    assert_eq!(status(&a)["session_tick"], 0);
    assert!(recording(&a).unwrap().frames.is_empty());
    assert!(!recording(&a).unwrap().truncated);
}
#[test]
fn scene_has_distinct_markers_fixed_camera_and_active_indicator() {
    let a = app();
    let scene = extract(a.world()).unwrap();
    assert!(scene.draws().len() >= 15);
    assert_eq!(scene.camera().position(), Vec3::new(6.0, 14.0, 17.0));
    assert!((scene.camera().vertical_fov_radians() - 50.0f32.to_radians()).abs() < 0.00001);
    assert!(
        scene
            .draws()
            .windows(2)
            .all(|p| p[0].draw().order < p[1].draw().order)
    );
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    assert_eq!(pos(&a, "strong"), initial_position(1));
}

#[test]
fn consecutive_switch_commands_are_recorded_as_distinct_edges() {
    let mut a = app();
    a.world_mut().insert_resource(PendingSwitch);
    a.advance_fixed(1);
    assert_eq!(status(&a)["active_character"], "strong");
    a.world_mut().insert_resource(PendingSwitch);
    a.advance_fixed(1);
    assert_eq!(status(&a)["active_character"], "jumper");
    let record = recording(&a).unwrap();
    replay(&mut a, record).unwrap();
    assert_eq!(status(&a)["active_character"], "jumper");
    assert_eq!(status(&a)["session_tick"], 2);
}

#[test]
fn fresh_press_unlocks_only_released_action_and_replays_exactly() {
    let mut a = app();
    let mut tracker = InputTracker::default();
    tick_with(&mut a, &mut tracker, &[Action::Right, Action::Up]);
    tick_with(
        &mut a,
        &mut tracker,
        &[Action::Right, Action::Up, Action::Switch],
    );
    let input = RecordedButtons {
        active: vec!["right".into(), "up".into(), "switch".into()],
        pressed: vec!["right".into()],
        released: vec![],
    }
    .decode(&SCHEMA)
    .unwrap();
    a.world_mut().insert_resource(input);
    a.advance_fixed(1);
    assert_eq!(
        pos(&a, "strong"),
        Position {
            x: 3560,
            y: 0,
            z: 6500
        }
    );
    let before = status(&a);
    assert_eq!(
        before["blocked_actions"],
        serde_json::json!(["up", "switch"])
    );
    let recording = recording(&a).unwrap();
    replay(&mut a, recording).unwrap();
    let after = status(&a);
    for key in [
        "characters",
        "active_character",
        "blocked_actions",
        "consumed_input",
    ] {
        assert_eq!(before[key], after[key], "{key}");
    }
}

#[test]
fn jumps_have_exact_distinct_apices_and_holding_never_repeats() {
    for (index, expected) in [(0, 1530), (1, 450)] {
        let mut a = app();
        a.world_mut().resource_mut::<Session>().unwrap().active = index;
        let mut t = InputTracker::default();
        let mut apex = 0;
        for _ in 0..90 {
            tick_with(&mut a, &mut t, &[Action::Jump]);
            apex = apex.max(pos(&a, character_name(index)).y);
        }
        assert_eq!(apex, expected);
        assert_eq!(pos(&a, character_name(index)).y, 0);
        tick_with(&mut a, &mut t, &[]);
        tick_with(&mut a, &mut t, &[Action::Jump]);
        assert!(pos(&a, character_name(index)).y > 0);
    }
}
#[test]
fn inactive_airborne_character_lands_without_horizontal_motion_or_held_jump_transfer() {
    let mut a = app();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Jump, Action::Right]);
    let x = pos(&a, "jumper").x;
    tick_with(
        &mut a,
        &mut t,
        &[Action::Jump, Action::Right, Action::Switch],
    );
    for _ in 0..45 {
        tick_with(&mut a, &mut t, &[Action::Jump, Action::Right]);
    }
    assert_eq!(pos(&a, "jumper"), Position { x, y: 0, z: 6500 });
    assert_eq!(pos(&a, "strong"), initial_position(1));
}
#[test]
fn jump_in_air_is_not_buffered_and_characters_do_not_support_each_other() {
    let mut a = app();
    let mut t = InputTracker::default();
    fixture_set_character(&mut a, 1, initial_position(0), 0, true);
    tick_with(&mut a, &mut t, &[Action::Jump]);
    tick_with(&mut a, &mut t, &[]);
    for _ in 0..60 {
        tick_with(&mut a, &mut t, &[Action::Jump]);
    }
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    assert_eq!(pos(&a, "strong"), initial_position(0));
}
#[test]
fn defensive_fall_resets_both_and_clears_pending_and_gates_held_input() {
    let mut a = build_recovery_fixture();
    let mut t = InputTracker::default();
    fixture_set_character(
        &mut a,
        0,
        Position {
            x: 1800,
            y: 0,
            z: 6500,
        },
        0,
        true,
    );
    a.world_mut()
        .resource_mut::<ScheduledInput>()
        .unwrap()
        .frames
        .insert(100, vec![(Action::Right, ActionValue::PRESSED)]);
    tick_with(
        &mut a,
        &mut t,
        &[Action::Right, Action::Jump, Action::Switch],
    );
    let s = status(&a);
    assert_eq!(s["session_generation"], 1);
    assert_eq!(s["session_tick"], 0);
    assert_eq!(s["recorded_ticks"], 0);
    assert_eq!(s["pending_inputs"], 0);
    assert_eq!(s["active_character"], "jumper");
    assert_eq!(s["recovery_message_ticks"], 120);
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    assert_eq!(pos(&a, "strong"), initial_position(1));
    tick_with(
        &mut a,
        &mut t,
        &[Action::Right, Action::Jump, Action::Switch],
    );
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    tick_with(&mut a, &mut t, &[]);
    tick_with(&mut a, &mut t, &[Action::Right, Action::Jump]);
    assert_eq!(pos(&a, "jumper").x, 1560);
    assert_eq!(pos(&a, "jumper").y, 170);
}
#[test]
fn ledges_block_walking_but_jumper_can_land_on_teaching_ledge() {
    for (index, expected_y) in [(0, 1000), (1, 0)] {
        let mut a = app();
        let mut t = InputTracker::default();
        fixture_set_character(
            &mut a,
            index,
            Position {
                x: 2000,
                y: 0,
                z: 3500,
            },
            0,
            true,
        );
        a.world_mut().resource_mut::<Session>().unwrap().active = index;
        for _ in 0..10 {
            tick_with(&mut a, &mut t, &[Action::Up]);
        }
        assert_eq!(pos(&a, character_name(index)).z, 3200);
        tick_with(&mut a, &mut t, &[Action::Up, Action::Jump]);
        for _ in 0..20 {
            tick_with(&mut a, &mut t, &[Action::Up]);
        }
        for _ in 0..30 {
            tick_with(&mut a, &mut t, &[]);
        }
        assert_eq!(pos(&a, character_name(index)).y, expected_y);
        assert!(
            status(&a)["characters"][character_name(index)]["grounded"]
                .as_bool()
                .unwrap()
        );
    }
}
#[test]
fn support_requires_positive_overlap_and_walkoff_gets_gravity_immediately() {
    use movement::*;
    for (x, expected) in [(3199, 1000), (3200, 990)] {
        let mut p = Position {
            x: 3100,
            y: 1000,
            z: 2000,
        };
        let mut m = Movement::default();
        advance(&mut p, &mut m, x - 3100, 0, false, 180, &SOLIDS);
        assert_eq!(p.y, expected);
        assert_eq!(m.grounded, x == 3199);
    }
}
#[test]
fn swept_contacts_choose_nearest_ceiling_highest_support_and_slide_x_then_z() {
    use movement::*;
    let mut p = Position {
        x: 2000,
        y: 3000,
        z: 2000,
    };
    let mut m = Movement {
        velocity_y: -5000,
        grounded: false,
        support: None,
        collisions: Default::default(),
    };
    advance(&mut p, &mut m, 0, 0, false, 180, &SOLIDS);
    assert_eq!(p.y, 1000);
    assert_eq!(m.support, Some("teaching-ledge"));
    let mut p = Position {
        x: 10000,
        y: 0,
        z: 5000,
    };
    let mut m = Movement::default();
    let ceiling = movement::solid("test-ceiling", (9000, 1300, 4500), (11000, 1550, 5500));
    let mut solids = SOLIDS.to_vec();
    solids.push(ceiling);
    advance(&mut p, &mut m, 0, 0, true, 3000, &solids);
    assert_eq!(p.y, 400);
    assert_eq!(m.velocity_y, 0);
    assert_eq!(m.collisions.ceiling, Some("test-ceiling"));
    let mut p = Position {
        x: 500,
        y: 0,
        z: 2000,
    };
    let mut m = Movement::default();
    advance(&mut p, &mut m, 5000, 60, false, 180, &SOLIDS);
    assert_eq!(p.x, 800);
    assert_eq!(p.z, 2060);
    assert_eq!(m.collisions.x, Some("teaching-ledge"));
}

#[test]
fn fresh_jump_edge_between_ticks_is_preserved_after_landing() {
    let mut a = app();
    let mut t = InputTracker::default();
    for _ in 0..45 {
        tick_with(&mut a, &mut t, &[Action::Jump]);
    }
    assert_eq!(pos(&a, "jumper").y, 0);
    let fresh = RecordedButtons {
        active: vec!["jump".into()],
        pressed: vec!["jump".into()],
        released: vec![],
    }
    .decode(&SCHEMA)
    .unwrap();
    a.world_mut().insert_resource(fresh);
    a.advance_fixed(1);
    assert_eq!(pos(&a, "jumper").y, 170);
    let before = status(&a);
    let r = recording(&a).unwrap();
    replay(&mut a, r).unwrap();
    assert_eq!(status(&a)["characters"], before["characters"]);
}
#[test]
fn post_recovery_recording_restores_held_gates_and_message_and_validates_before_reset() {
    let mut a = build_recovery_fixture();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Right, Action::Jump]);
    for _ in 0..5 {
        tick_with(&mut a, &mut t, &[Action::Right, Action::Jump]);
    }
    let before = status(&a);
    let r = recording(&a).unwrap();
    replay(&mut a, r).unwrap();
    for key in [
        "characters",
        "blocked_actions",
        "recovery_message_ticks",
        "session_tick",
        "consumed_input",
        "recorded_ticks",
    ] {
        assert_eq!(status(&a)[key], before[key], "{key}");
    }
    let mut r = recording(&a).unwrap();
    r.origin.blocked_actions.push("invalid".into());
    let before = status(&a);
    assert!(replay(&mut a, r).is_err());
    assert_eq!(status(&a), before);
}

fn inspect(app: &mut App, inspector: &mut Inspector, request: titan_protocol::Request) {
    use titan_protocol::{RequestEnvelope, ResponseOutcome};
    let response = inspector.handle(app, &RequestEnvelope::new("injected-regression", request));
    assert!(
        matches!(response.outcome, ResponseOutcome::Success { .. }),
        "{response:?}"
    );
}
fn injected(app: &mut App, inspector: &mut Inspector, actions: &[&str]) {
    use titan_protocol::Request;
    let frame = app.world().resource::<FixedTime>().unwrap().tick() + 1;
    inspect(
        app,
        inspector,
        Request::InjectInput {
            frame,
            actions: actions
                .iter()
                .map(|name| (name.to_string(), InputValue::Button(true)))
                .collect(),
        },
    );
    inspect(app, inspector, Request::Step { frames: 1 });
}
#[test]
fn inspector_injected_holds_cannot_bypass_restart_or_recovery_release_gates() {
    for boundary in ["key-restart", "command-restart", "fall"] {
        let mut a = app();
        let mut inspector =
            configured_inspector(InspectionConfig::controlled("input-test", "adventure"));
        if boundary == "key-restart" {
            injected(
                &mut a,
                &mut inspector,
                &["restart", "jump", "right", "switch"],
            );
        } else {
            injected(&mut a, &mut inspector, &["jump", "right", "switch"]);
            if boundary == "fall" {
                fixture_set_character(
                    &mut a,
                    1,
                    Position {
                        x: 3500,
                        y: -2000,
                        z: 6500,
                    },
                    -10,
                    false,
                );
                injected(&mut a, &mut inspector, &["jump", "right", "switch"]);
            } else {
                restart(&mut a);
            }
        }
        let generation = status(&a)["session_generation"].clone();
        // No sampled release: independent snapshots must still mean held.
        let held = if boundary == "key-restart" {
            vec!["restart", "jump", "right", "switch"]
        } else {
            vec!["jump", "right", "switch"]
        };
        for _ in 0..3 {
            injected(&mut a, &mut inspector, &held);
        }
        assert_eq!(status(&a)["session_generation"], generation, "{boundary}");
        assert_eq!(status(&a)["active_character"], "jumper", "{boundary}");
        assert_eq!(pos(&a, "jumper"), initial_position(0), "{boundary}");
        let expected = status(&a);
        let r = recording(&a).unwrap();
        replay(&mut a, r).unwrap();
        for key in [
            "characters",
            "active_character",
            "consumed_input",
            "session_tick",
        ] {
            assert_eq!(status(&a)[key], expected[key], "{boundary}:{key}");
        }
        injected(&mut a, &mut inspector, &[]);
        injected(&mut a, &mut inspector, &["jump", "right"]);
        assert_eq!(pos(&a, "jumper").y, 170);
        assert_eq!(pos(&a, "jumper").x, 1560);
    }
}

#[test]
fn clearing_pending_injection_preserves_release_gate_across_other_source_ticks() {
    let mut a = app();
    let mut inspector =
        configured_inspector(InspectionConfig::controlled("clear-test", "adventure"));
    injected(&mut a, &mut inspector, &["right"]);
    assert_eq!(pos(&a, "jumper").x, 1560);
    // This is the input-source clear used by player pause/resume. An intervening
    // keyboard/empty frame must not count as release by the injected source.
    clear_scheduled_input(a.world_mut());
    a.world_mut()
        .insert_resource(InputFrame::<Action>::default());
    a.advance_fixed(1);
    injected(&mut a, &mut inspector, &["right"]);
    assert_eq!(pos(&a, "jumper").x, 1560);
    injected(&mut a, &mut inspector, &[]);
    injected(&mut a, &mut inspector, &["right"]);
    assert_eq!(pos(&a, "jumper").x, 1620);
}

#[test]
fn plates_require_grounded_centers_at_support_height_and_include_boundaries() {
    for (x, y, grounded, expected) in [
        (1700, 1000, true, true),
        (2300, 1000, true, true),
        (1699, 1000, true, false),
        (2301, 1000, true, false),
        (2000, 1001, true, false),
        (2000, 1000, false, false),
    ] {
        let mut puzzle = puzzle::PuzzleState::default();
        let movement = Movement {
            grounded,
            ..Movement::default()
        };
        puzzle.sample([
            (Position { x, y, z: 2000 }, movement),
            (initial_position(1), Movement::default()),
        ]);
        assert_eq!(puzzle.plates[0].pressed, expected);
        assert_eq!(puzzle.door.open, expected);
    }
    let mut puzzle = puzzle::PuzzleState::default();
    puzzle.sample(
        [(
            Position {
                x: 2000,
                y: 1000,
                z: 2000,
            },
            Movement::default(),
        ); 2],
    );
    assert_eq!(puzzle.plates[0].occupants, vec!["jumper", "strong"]);
}
#[test]
fn either_plate_holds_door_and_positive_airborne_obstruction_is_safe() {
    let mut puzzle = puzzle::PuzzleState::default();
    for (a, b) in [(true, false), (false, true), (true, true), (false, false)] {
        puzzle.sample([
            (
                if a {
                    Position {
                        x: 2000,
                        y: 1000,
                        z: 2000,
                    }
                } else {
                    initial_position(0)
                },
                Movement::default(),
            ),
            (
                if b {
                    Position {
                        x: 10000,
                        y: 0,
                        z: 5000,
                    }
                } else {
                    initial_position(1)
                },
                Movement::default(),
            ),
        ]);
        assert_eq!(puzzle.door.open, a || b);
        assert_eq!(
            puzzle.door.state,
            if a || b { "open_plate" } else { "closed" }
        );
    }
    for (x, expected) in [(6800, false), (6801, true), (8200, false), (8199, true)] {
        let airborne = Movement {
            grounded: false,
            ..Movement::default()
        };
        puzzle.sample([
            (
                Position {
                    x,
                    y: 1000,
                    z: 5000,
                },
                airborne,
            ),
            (initial_position(1), Movement::default()),
        ]);
        assert_eq!(
            puzzle.door.state,
            if expected {
                "open_obstructed"
            } else {
                "closed"
            }
        );
    }
}
#[test]
fn door_collision_uses_previous_tick_and_inactive_character_holds_plate() {
    let mut a = app();
    let mut t = InputTracker::default();
    fixture_set_character(
        &mut a,
        0,
        Position {
            x: 2000,
            y: 1000,
            z: 2000,
        },
        0,
        true,
    );
    fixture_set_character(
        &mut a,
        1,
        Position {
            x: 6800,
            y: 0,
            z: 5000,
        },
        0,
        true,
    );
    a.world_mut().resource_mut::<Session>().unwrap().active = 1;
    tick_with(&mut a, &mut t, &[Action::Right]);
    assert_eq!(pos(&a, "strong").x, 6800);
    assert_eq!(status(&a)["puzzle"]["door"]["state"], "open_plate");
    tick_with(&mut a, &mut t, &[Action::Right]);
    assert_eq!(pos(&a, "strong").x, 6860);
    assert_eq!(
        status(&a)["puzzle"]["plates"][0]["occupants"],
        serde_json::json!(["jumper"])
    );
    fixture_set_character(&mut a, 0, initial_position(0), 0, true);
    tick_with(&mut a, &mut t, &[Action::Right]);
    assert_eq!(status(&a)["puzzle"]["door"]["state"], "open_obstructed");
    for _ in 0..23 {
        tick_with(&mut a, &mut t, &[Action::Right]);
    }
    assert_eq!(status(&a)["puzzle"]["door"]["state"], "closed");
}
#[test]
fn exit_requires_both_full_grounded_footprints_and_completion_freezes_until_restart() {
    let mut puzzle = puzzle::PuzzleState::default();
    let inside = (
        Position {
            x: 10200,
            y: 0,
            z: 1200,
        },
        Movement::default(),
    );
    for (p, grounded) in [
        (
            Position {
                x: 10199,
                y: 0,
                z: 1200,
            },
            true,
        ),
        (
            Position {
                x: 10200,
                y: 0,
                z: 1199,
            },
            true,
        ),
        (
            Position {
                x: 10200,
                y: 1,
                z: 1200,
            },
            false,
        ),
        (inside.0, false),
    ] {
        let m = Movement {
            grounded,
            ..Movement::default()
        };
        puzzle.sample([inside, (p, m)]);
        assert!(puzzle.exit.jumper);
        assert!(!puzzle.complete);
    }
    puzzle.sample([(initial_position(0), Movement::default()), inside]);
    assert!(!puzzle.complete);
    assert!(!puzzle.exit.jumper);
    assert!(puzzle.exit.strong);
    let mut a = app();
    let mut t = InputTracker::default();
    for i in 0..2 {
        fixture_set_character(&mut a, i, inside.0, 0, true);
    }
    tick_with(&mut a, &mut t, &[]);
    let before = status(&a);
    assert_eq!(before["puzzle"]["complete"], true);
    tick_with(
        &mut a,
        &mut t,
        &[Action::Right, Action::Jump, Action::Switch],
    );
    for key in ["characters", "puzzle", "active_character", "session_tick"] {
        assert_eq!(status(&a)[key], before[key]);
    }
    tick_with(
        &mut a,
        &mut t,
        &[Action::Right, Action::Jump, Action::Switch, Action::Restart],
    );
    assert_eq!(status(&a)["puzzle"]["complete"], false);
    assert_eq!(status(&a)["puzzle"]["door"]["state"], "closed");
    tick_with(
        &mut a,
        &mut t,
        &[Action::Right, Action::Jump, Action::Switch, Action::Restart],
    );
    assert_eq!(pos(&a, "jumper"), initial_position(0));
}

#[test]
fn room_two_reset_replay_and_input_edges() {
    let mut a = app();
    select_room(&mut a, 2).unwrap();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Switch]);
    for _ in 0..33 {
        tick_with(&mut a, &mut t, &[Action::Right]);
    }
    tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    assert_eq!(status(&a)["block"]["socket"], 1);
    assert_eq!(
        pos(&a, "strong"),
        Position {
            x: 5480,
            y: 0,
            z: 6500
        }
    );
    for _ in 0..16 {
        tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    }
    assert_eq!(status(&a)["block"]["moves"], 1);
    tick_with(&mut a, &mut t, &[]);
    tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    assert_eq!(status(&a)["block"]["socket"], 2);
    let before = status(&a);
    let rec = recording(&a).unwrap();
    select_room(&mut a, 1).unwrap();
    replay(&mut a, rec).unwrap();
    let after = status(&a);
    for key in [
        "room",
        "block",
        "characters",
        "puzzle",
        "session_tick",
        "active_character",
    ] {
        assert_eq!(before[key], after[key], "{key}");
    }
    tick_with(&mut a, &mut t, &[Action::Restart]);
    assert_eq!(status(&a)["room"], 2);
    assert_eq!(status(&a)["block"]["socket"], 0);
    assert_eq!(status(&a)["active_character"], "jumper");
    assert!(select_room(&mut a, 3).is_err());
    assert_eq!(status(&a)["room"], 2);
}
#[test]
fn room_two_recovery_restores_block_and_room() {
    let mut a = app();
    select_room(&mut a, 2).unwrap();
    a.world_mut()
        .resource_mut::<Session>()
        .unwrap()
        .block
        .socket = 2;
    fixture_set_character(
        &mut a,
        1,
        Position {
            x: 3500,
            y: -2001,
            z: 6500,
        },
        -10,
        false,
    );
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    let s = status(&a);
    assert_eq!(s["room"], 2);
    assert_eq!(s["block"]["socket"], 0);
    assert_eq!(s["recovery_message_ticks"], 120);
    assert_eq!(pos(&a, "strong"), initial_position(1));
    let recording = recording(&a).unwrap();
    select_room(&mut a, 1).unwrap();
    replay(&mut a, recording).unwrap();
    assert_eq!(status(&a)["room"], 2);
    assert_eq!(status(&a)["recovery_message_ticks"], 120);
}

#[test]
fn room_two_push_uses_resolved_axes_and_rejections_allow_ordinary_movement() {
    let mut a = app();
    select_room(&mut a, 2).unwrap();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Switch]);
    fixture_set_character(
        &mut a,
        1,
        Position {
            x: 5500,
            y: 0,
            z: 6500,
        },
        0,
        true,
    );
    tick_with(
        &mut a,
        &mut t,
        &[Action::Interact, Action::Up, Action::Left, Action::Right],
    );
    assert_eq!(status(&a)["block"]["socket"], 1);
    tick_with(&mut a, &mut t, &[]);
    fixture_set_character(
        &mut a,
        1,
        Position {
            x: 5500,
            y: 0,
            z: 5500,
        },
        0,
        true,
    );
    tick_with(
        &mut a,
        &mut t,
        &[Action::Interact, Action::Up, Action::Jump],
    );
    assert_eq!(status(&a)["block"]["last_rejection"], "not_grounded");
    assert_eq!(
        pos(&a, "strong"),
        Position {
            x: 5500,
            y: 90,
            z: 5440
        }
    );
    assert_eq!(status(&a)["block"]["socket"], 1);
}
#[test]
fn dynamic_block_support_stays_stable_and_prevents_push() {
    let mut a = app();
    select_room(&mut a, 2).unwrap();
    fixture_set_character(
        &mut a,
        0,
        Position {
            x: 5500,
            y: 750,
            z: 5500,
        },
        0,
        true,
    );
    fixture_set_character(
        &mut a,
        1,
        Position {
            x: 5500,
            y: 0,
            z: 6500,
        },
        0,
        true,
    );
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Switch]);
    for _ in 0..40 {
        tick_with(&mut a, &mut t, &[]);
    }
    assert_eq!(status(&a)["characters"]["jumper"]["support"], "heavy-block");
    tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    assert_eq!(status(&a)["block"]["last_rejection"], "block_occupied");
    assert_eq!(status(&a)["block"]["socket"], 0);
    assert_eq!(
        pos(&a, "jumper"),
        Position {
            x: 5500,
            y: 750,
            z: 5500
        }
    );
    // Switch-held E cannot become a fresh push after changing control target.
    tick_with(
        &mut a,
        &mut t,
        &[Action::Interact, Action::Up, Action::Switch],
    );
    assert_eq!(status(&a)["active_character"], "jumper");
    assert_eq!(status(&a)["block"]["last_rejection"], "block_occupied");
    assert_eq!(
        pos(&a, "jumper"),
        Position {
            x: 5500,
            y: 750,
            z: 5500
        }
    );
}

fn finish_room_fixture(a: &mut App, t: &mut InputTracker<Action>) {
    for index in 0..2 {
        fixture_set_character(
            a,
            index,
            Position {
                x: 10_500,
                y: 0,
                z: 1500,
            },
            0,
            true,
        );
    }
    tick_with(a, t, &[]);
    assert_eq!(status(a)["puzzle"]["complete"], true);
}

#[test]
fn start_freezes_and_confirm_reconstructs_with_all_held_actions_gated() {
    let mut a = build_game();
    a.update_schedule(Startup);
    let mut t = InputTracker::default();
    let held = [
        Action::Right,
        Action::Jump,
        Action::Interact,
        Action::Switch,
    ];
    tick_with(&mut a, &mut t, &held);
    assert_eq!(status(&a)["phase"], "start");
    assert_eq!(status(&a)["session_tick"], 0);
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    let mut start = held.to_vec();
    start.push(Action::Confirm);
    tick_with(&mut a, &mut t, &start);
    assert_eq!(status(&a)["phase"], "playing");
    assert_eq!(status(&a)["session_generation"], 1);
    assert_eq!(status(&a)["session_tick"], 0);
    tick_with(&mut a, &mut t, &start);
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    assert_eq!(status(&a)["active_character"], "jumper");
    tick_with(&mut a, &mut t, &[]);
    tick_with(&mut a, &mut t, &[Action::Right, Action::Jump]);
    assert_eq!(pos(&a, "jumper").x, 1560);
    assert_eq!(pos(&a, "jumper").y, 170);
    let expected = status(&a);
    let record = recording(&a).unwrap();
    assert_eq!(record.origin.phase, Phase::Start);
    assert_eq!(record.frames.len(), 5);
    replay(&mut a, record).unwrap();
    for key in [
        "phase",
        "characters",
        "active_character",
        "consumed_input",
        "session_tick",
        "blocked_actions",
        "recorded_ticks",
    ] {
        assert_eq!(status(&a)[key], expected[key], "{key}");
    }
}

#[test]
fn completion_requires_fresh_confirm_and_rebuilds_each_destination() {
    let mut a = app();
    let mut t = InputTracker::default();
    // A confirm held during play must not skip the completion prompt.
    tick_with(&mut a, &mut t, &[Action::Confirm]);
    for index in 0..2 {
        fixture_set_character(
            &mut a,
            index,
            Position {
                x: 10_500,
                y: 0,
                z: 1500,
            },
            0,
            true,
        );
    }
    tick_with(&mut a, &mut t, &[Action::Confirm]);
    assert_eq!(status(&a)["phase"], "room_complete");
    let completed = status(&a);
    tick_with(
        &mut a,
        &mut t,
        &[Action::Confirm, Action::Right, Action::Jump, Action::Switch],
    );
    assert_eq!(status(&a)["characters"], completed["characters"]);
    assert_eq!(status(&a)["session_tick"], completed["session_tick"]);
    tick_with(&mut a, &mut t, &[]);
    tick_with(
        &mut a,
        &mut t,
        &[
            Action::Confirm,
            Action::Right,
            Action::Jump,
            Action::Switch,
            Action::Interact,
        ],
    );
    assert_eq!(status(&a)["room"], 2);
    assert_eq!(status(&a)["phase"], "playing");
    assert_eq!(status(&a)["session_tick"], 0);
    assert_eq!(status(&a)["active_character"], "jumper");
    assert_eq!(status(&a)["puzzle"]["complete"], false);
    for index in 0..2 {
        assert_eq!(pos(&a, character_name(index)), initial_position(index));
    }
    tick_with(
        &mut a,
        &mut t,
        &[
            Action::Confirm,
            Action::Right,
            Action::Jump,
            Action::Switch,
            Action::Interact,
        ],
    );
    assert_eq!(pos(&a, "jumper"), initial_position(0));
    assert_eq!(status(&a)["active_character"], "jumper");
    finish_room_fixture(&mut a, &mut t);
    assert_eq!(status(&a)["phase"], "slice_complete");
    let before = status(&a);
    tick_with(&mut a, &mut t, &[Action::Right, Action::Switch]);
    assert_eq!(status(&a)["characters"], before["characters"]);
    confirm(&mut a);
    assert_eq!(status(&a)["room"], 1);
    assert_eq!(status(&a)["phase"], "playing");
    assert_eq!(status(&a)["session_tick"], 0);
    assert_eq!(status(&a)["active_character"], "jumper");
    assert_eq!(recording(&a).unwrap().room, 1);
}

#[test]
fn restart_from_every_phase_keeps_displayed_room_and_canonical_playing_origin() {
    for (phase, room) in [
        (Phase::Start, 1),
        (Phase::Playing, 1),
        (Phase::RoomComplete, 1),
        (Phase::Playing, 2),
        (Phase::SliceComplete, 2),
    ] {
        let mut a = app();
        select_room(&mut a, room).unwrap();
        a.world_mut().resource_mut::<Session>().unwrap().phase = phase;
        let mut t = InputTracker::default();
        tick_with(
            &mut a,
            &mut t,
            &[
                Action::Restart,
                Action::Right,
                Action::Jump,
                Action::Switch,
                Action::Confirm,
            ],
        );
        assert_eq!(status(&a)["room"], room);
        assert_eq!(status(&a)["phase"], "playing");
        assert_eq!(status(&a)["session_tick"], 0);
        tick_with(
            &mut a,
            &mut t,
            &[
                Action::Restart,
                Action::Right,
                Action::Jump,
                Action::Switch,
                Action::Confirm,
            ],
        );
        assert_eq!(pos(&a, "jumper"), initial_position(0));
        assert_eq!(status(&a)["active_character"], "jumper");
        let before = status(&a);
        let r = recording(&a).unwrap();
        assert_eq!(r.origin.phase, Phase::Playing);
        assert_eq!(r.room, room);
        replay(&mut a, r).unwrap();
        for key in [
            "room",
            "phase",
            "characters",
            "session_tick",
            "blocked_actions",
        ] {
            assert_eq!(status(&a)[key], before[key], "{phase:?}:{key}");
        }
    }
}

#[test]
fn injected_start_and_continue_clear_future_inputs_and_preserve_release_gates() {
    for phase in [Phase::Start, Phase::RoomComplete, Phase::SliceComplete] {
        let mut a = app();
        a.world_mut().resource_mut::<Session>().unwrap().phase = phase;
        let mut inspector =
            configured_inspector(InspectionConfig::controlled("sequence-test", "adventure"));
        let now = status(&a)["frame"].as_u64().unwrap();
        inspect(
            &mut a,
            &mut inspector,
            titan_protocol::Request::InjectInput {
                frame: now + 2,
                actions: BTreeMap::from([("right".into(), InputValue::Button(true))]),
            },
        );
        injected(
            &mut a,
            &mut inspector,
            &["confirm", "switch", "right", "jump", "interact"],
        );
        assert_eq!(status(&a)["pending_inputs"], 0);
        assert_eq!(status(&a)["phase"], "playing");
        assert_eq!(
            status(&a)["room"],
            if phase == Phase::RoomComplete { 2 } else { 1 }
        );
        for _ in 0..3 {
            injected(
                &mut a,
                &mut inspector,
                &["confirm", "switch", "right", "jump", "interact"],
            );
        }
        assert_eq!(pos(&a, "jumper"), initial_position(0));
        assert_eq!(status(&a)["active_character"], "jumper");
        injected(&mut a, &mut inspector, &[]);
        injected(&mut a, &mut inspector, &["right", "jump"]);
        assert_eq!(pos(&a, "jumper").x, 1560);
        assert_eq!(pos(&a, "jumper").y, 170);
    }
}

#[test]
fn invalid_completion_origin_is_rejected_before_mutation_and_legacy_origin_defaults_to_playing() {
    let mut a = app();
    let json = serde_json::json!({"blocked_actions": [], "recovery_message_ticks": 0});
    let origin: RecordingOrigin = serde_json::from_value(json).unwrap();
    assert_eq!(origin.phase, Phase::Playing);
    let mut r = recording(&a).unwrap();
    r.origin.phase = Phase::SliceComplete;
    let before = status(&a);
    assert!(replay(&mut a, r).is_err());
    assert_eq!(status(&a), before);
}

#[test]
fn successful_push_accepts_between_tick_release_repress_edge() {
    let mut a = app();
    select_room(&mut a, 2).unwrap();
    let mut t = InputTracker::default();
    tick_with(&mut a, &mut t, &[Action::Switch]);
    fixture_set_character(
        &mut a,
        1,
        Position {
            x: 5500,
            y: 0,
            z: 6150,
        },
        0,
        true,
    );
    tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    for _ in 0..30 {
        tick_with(&mut a, &mut t, &[Action::Interact, Action::Up]);
    }
    assert_eq!(status(&a)["block"]["moves"], 1);
    let fresh = RecordedButtons {
        active: vec!["interact".into(), "up".into()],
        pressed: vec!["interact".into()],
        released: vec![],
    }
    .decode(&SCHEMA)
    .unwrap();
    a.world_mut().insert_resource(fresh);
    a.advance_fixed(1);
    assert_eq!(status(&a)["block"]["moves"], 2);
}
