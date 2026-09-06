use super::*;
fn app() -> App {
    let mut app = build_game();
    app.update_schedule(Startup);
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
            x: 11800,
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
    assert_eq!(scene.draws().len(), 16);
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
    advance(&mut p, &mut m, 0, 0, true, 3000, &SOLIDS);
    assert_eq!(p.y, 400);
    assert_eq!(m.velocity_y, 0);
    assert_eq!(m.collisions.ceiling, Some("practice-ceiling"));
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
