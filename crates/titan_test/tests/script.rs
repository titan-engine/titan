//! Script serialization, absolute tick playback, and seeded gameplay replay.

use bevy_app::{Startup, Update};
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput};
use titan_test::{ExecutorKind, InputAction, InputButton, InputScript, ScriptEvent, Sim, SimSeed};

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputFrame {
    key_pressed: bool,
    key_just_pressed: bool,
    key_just_released: bool,
    mouse_pressed: bool,
    mouse_just_pressed: bool,
    mouse_just_released: bool,
}

#[derive(Resource, Default)]
struct History(Vec<InputFrame>);

fn record(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut history: ResMut<History>,
) {
    history.0.push(InputFrame {
        key_pressed: keys.pressed(KeyCode::Space),
        key_just_pressed: keys.just_pressed(KeyCode::Space),
        key_just_released: keys.just_released(KeyCode::Space),
        mouse_pressed: mouse.pressed(MouseButton::Left),
        mouse_just_pressed: mouse.just_pressed(MouseButton::Left),
        mouse_just_released: mouse.just_released(MouseButton::Left),
    });
}

fn recording_sim() -> Sim {
    Sim::new(|app| {
        app.init_resource::<History>().add_systems(Update, record);
    })
}

fn script() -> InputScript {
    InputScript {
        version: 1,
        events: vec![
            ScriptEvent {
                tick: 0,
                action: InputAction::Press(InputButton::Key(KeyCode::Space)),
            },
            ScriptEvent {
                tick: 2,
                action: InputAction::Release(InputButton::Key(KeyCode::Space)),
            },
            ScriptEvent {
                tick: 3,
                action: InputAction::Tap(InputButton::Mouse(MouseButton::Left)),
            },
        ],
    }
}

#[test]
fn public_script_fields_roundtrip_through_ron() {
    let original = script();
    let text = original.to_ron().unwrap();
    let decoded = InputScript::from_ron(&text).unwrap();
    assert_eq!(decoded.version, original.version);
    assert_eq!(decoded.events.len(), original.events.len());
    for (actual, expected) in decoded.events.iter().zip(&original.events) {
        assert_eq!(actual.tick, expected.tick);
        assert_eq!(actual.action, expected.action);
    }
    assert!(InputScript::from_ron("this is not RON").is_err());
    let mut before_roundtrip = recording_sim();
    let mut after_roundtrip = recording_sim();
    before_roundtrip.run_script(&original, 6);
    after_roundtrip.run_script(&decoded, 6);
    assert_eq!(
        before_roundtrip.resource::<History>().0,
        after_roundtrip.resource::<History>().0
    );
}

#[test]
fn documented_ron_syntax_deserializes() {
    let script = InputScript::from_ron(
        "(version:1,events:[(tick:10,action:Press(Key(Space))),(tick:30,action:Tap(Mouse(Left)))])",
    )
    .unwrap();
    assert_eq!(script.version, 1);
    assert_eq!(script.events[0].tick, 10);
    assert_eq!(
        script.events[0].action,
        InputAction::Press(InputButton::Key(KeyCode::Space))
    );
    assert_eq!(script.events[1].tick, 30);
    assert_eq!(
        script.events[1].action,
        InputAction::Tap(InputButton::Mouse(MouseButton::Left))
    );
}

#[test]
fn script_ticks_are_absolute_exclusive_and_support_continuation() {
    let script = script();
    let mut split = recording_sim();
    split.run_script(&script, 3);
    assert_eq!(split.current_tick(), 3);
    assert_eq!(split.resource::<History>().0.len(), 3);
    // Tick 3 has not run, so its mouse tap has not been injected yet.
    assert!(!split.resource::<History>().0[2].mouse_pressed);
    split.run_script(&script, 4);
    assert_eq!(
        split.current_tick(),
        4,
        "a scripted tap must not add an extra update"
    );
    assert!(split.resource::<History>().0[3].mouse_just_pressed);
    split.run_script(&script, 6);
    assert_eq!(split.current_tick(), 6);
    assert!(split.resource::<History>().0[4].mouse_just_released);
    assert!(!split.resource::<History>().0[5].mouse_just_released);

    let mut uninterrupted = recording_sim();
    uninterrupted.run_script(&script, 6);
    assert_eq!(
        split.resource::<History>().0,
        uninterrupted.resource::<History>().0
    );
    assert!(split.resource::<History>().0[0].key_just_pressed);
    assert!(split.resource::<History>().0[1].key_pressed);
    assert!(!split.resource::<History>().0[1].key_just_pressed);
    assert!(split.resource::<History>().0[2].key_just_released);

    split.run_script(&script, 6);
    assert_eq!(split.current_tick(), 6);
    assert_eq!(split.resource::<History>().0.len(), 6);
}

#[test]
fn script_ignores_events_before_the_current_tick() {
    let mut sim = recording_sim();
    sim.run_ticks(3);
    sim.run_script(&script(), 5);
    let history = &sim.resource::<History>().0;
    assert!(history
        .iter()
        .all(|frame| !frame.key_pressed && !frame.key_just_released));
    assert!(history[3].mouse_just_pressed);
    assert!(history[4].mouse_just_released);
}

#[test]
fn a_scripted_tap_can_finish_with_an_ordinary_tick() {
    let mut sim = recording_sim();
    sim.run_script(&script(), 4);
    assert!(sim.resource::<History>().0[3].mouse_pressed);
    sim.tick();
    assert!(sim.resource::<History>().0[4].mouse_just_released);
}

#[test]
fn unsorted_events_replay_in_tick_order() {
    let mut reversed = script();
    reversed.events.reverse();
    let mut first = recording_sim();
    let mut second = recording_sim();
    first.run_script(&script(), 6);
    second.run_script(&reversed, 6);
    assert_eq!(
        first.resource::<History>().0,
        second.resource::<History>().0
    );
}

#[test]
fn equal_tick_events_keep_file_order() {
    let key = InputButton::Key(KeyCode::Space);
    let mut script = InputScript {
        version: 1,
        events: vec![
            ScriptEvent {
                tick: 0,
                action: InputAction::Press(key),
            },
            ScriptEvent {
                tick: 0,
                action: InputAction::Release(key),
            },
        ],
    };
    let mut first = recording_sim();
    first.run_script(&script, 1);
    assert!(!first.resource::<History>().0[0].key_pressed);

    script.events.reverse();
    let mut second = recording_sim();
    second.run_script(&script, 1);
    assert!(second.resource::<History>().0[0].key_pressed);
}

#[test]
fn public_validation_accepts_supported_versions_and_rejects_unknown_versions() {
    let mut script = script();
    script.validate();
    script.version = titan_test::SCRIPT_VERSION;
    script.validate();
    for version in [0, titan_test::SCRIPT_VERSION + 1, u32::MAX] {
        script.version = version;
        assert!(std::panic::catch_unwind(|| script.validate()).is_err());
    }
}

#[test]
fn unknown_fields_and_unsupported_versions_do_not_silently_play() {
    assert!(InputScript::from_ron("(version:1,events:[],typo:0)").is_err());
    assert!(
        InputScript::from_ron("(version:1,events:[(tick:0,action:Press(Key(Space)),typo:0)])")
            .is_err()
    );
    let mut sim = recording_sim();
    let unsupported = InputScript {
        version: 3,
        events: vec![],
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sim.run_script(&unsupported, 3);
    }));
    assert!(result.is_err());
    assert_eq!(sim.current_tick(), 0);
    assert!(sim.resource::<History>().0.is_empty());
}

#[derive(Resource, Debug, Clone, PartialEq, Eq)]
struct SeededGame {
    offset: u64,
    actions: u64,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct Position(u64);

fn spawn_seeded_game(mut commands: Commands, seed: Res<SimSeed>) {
    // Deliberately use a local deterministic algorithm rather than a global RNG.
    let offset = seed.0.wrapping_mul(6364136223846793005).wrapping_add(1);
    commands.insert_resource(SeededGame { offset, actions: 0 });
    commands.spawn(Position(offset));
}

fn play_seeded_game(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut game: ResMut<SeededGame>,
    mut positions: Query<&mut Position>,
) {
    let step = u64::from(keys.pressed(KeyCode::Space))
        + 10 * u64::from(mouse.just_pressed(MouseButton::Left));
    game.actions += step;
    for mut position in &mut positions {
        position.0 = position.0.wrapping_add(step);
    }
}

fn replay(seed: u64) -> (SeededGame, Vec<Position>) {
    let mut sim = Sim::new(|app| {
        app.add_systems(Startup, spawn_seeded_game)
            .add_systems(Update, play_seeded_game);
    })
    .with_seed(seed)
    .with_fixed_dt(0.02)
    .with_executor_kind(ExecutorKind::SingleThreaded);
    assert_eq!(sim.resource::<SimSeed>().0, seed);
    sim.run_script(&script(), 8);
    let game = sim.resource::<SeededGame>().clone();
    let positions = sim.query::<&Position, ()>().into_iter().cloned().collect();
    (game, positions)
}

#[test]
fn identical_seed_and_script_replay_resource_and_component_state() {
    let first = replay(42);
    let second = replay(42);
    assert_eq!(first, second);
    assert_eq!(first.0.actions, 12);
    assert_eq!(first.1[0].0, first.0.offset.wrapping_add(12));
    assert_ne!(first, replay(43), "the game must actually consume SimSeed");
}
