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
    assert_eq!(pos(&a, "strong"), Position { x: 3500, z: 6440 });
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
    assert_eq!(pos(&a, "jumper"), Position { x: 1542, z: 6458 });
    tick_with(&mut a, &mut t, &[Action::Left, Action::Right]);
    assert_eq!(pos(&a, "jumper"), Position { x: 1542, z: 6458 });
    for _ in 0..250 {
        tick_with(&mut a, &mut t, &[Action::Up, Action::Left]);
    }
    assert_eq!(pos(&a, "jumper"), Position { x: 200, z: 200 });
    for _ in 0..300 {
        tick_with(&mut a, &mut t, &[Action::Down, Action::Right]);
    }
    assert_eq!(pos(&a, "jumper"), Position { x: 11800, z: 7800 });
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
    assert_eq!(scene.draws().len(), 12);
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
