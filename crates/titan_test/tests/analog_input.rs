//! Virtual controllers and raw mouse motion use Bevy's real input pipeline.

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{
    gamepad::{
        ButtonSettings, Gamepad, GamepadAxis, GamepadAxisChangedEvent, GamepadButton,
        GamepadConnectionEvent, GamepadSettings, RawGamepadAxisChangedEvent,
        RawGamepadButtonChangedEvent, RawGamepadEvent,
    },
    mouse::AccumulatedMouseMotion,
    ButtonInput,
};
use bevy_math::Vec2;
use std::panic::{catch_unwind, AssertUnwindSafe};
use titan_test::{GamepadSlot, InputAction, InputScript, ScriptEvent, Sim, SCRIPT_VERSION};

#[derive(Debug, Clone, PartialEq)]
struct Frame {
    pads: Vec<(String, bool, bool, bool, f32)>,
    motion: Vec2,
}

#[derive(Resource, Default)]
struct Game {
    history: Vec<Frame>,
    position: f32,
    look: Vec2,
    jumps: u32,
}

fn play(
    pads: Query<(&Name, &Gamepad)>,
    motion: Res<AccumulatedMouseMotion>,
    mut game: ResMut<Game>,
) {
    let mut frame = Frame {
        pads: Vec::new(),
        motion: motion.delta,
    };
    for (name, pad) in &pads {
        // In Bevy 0.20 ButtonInput<GamepadButton> is per Gamepad, not a resource.
        let buttons: &ButtonInput<GamepadButton> = pad.digital();
        let x = pad.get(GamepadAxis::LeftStickX).unwrap();
        game.position += x;
        game.jumps += u32::from(buttons.just_pressed(GamepadButton::South));
        frame.pads.push((
            name.as_str().to_owned(),
            buttons.pressed(GamepadButton::South),
            buttons.just_pressed(GamepadButton::South),
            buttons.just_released(GamepadButton::South),
            x,
        ));
    }
    frame.pads.sort_by(|a, b| a.0.cmp(&b.0));
    game.look += motion.delta;
    game.history.push(frame);
}

fn sim() -> Sim {
    Sim::new(|app| {
        app.init_resource::<Game>().add_systems(Update, play);
    })
}

#[test]
fn helpers_drive_a_game_with_independent_pads_held_axes_and_one_frame_motion() {
    let mut sim = sim();
    let first = sim.connect_gamepad();
    let second = sim.connect_gamepad();
    assert_eq!((first, second), (GamepadSlot(0), GamepadSlot(1)));
    let entity = sim.gamepad_entity(first).unwrap();
    assert_ne!(Some(entity), sim.gamepad_entity(second));
    assert!(sim.world().get::<Gamepad>(entity).is_none());
    assert_eq!(sim.current_tick(), 0);
    sim.press(first.button(GamepadButton::South));
    sim.set_axis(first, GamepadAxis::LeftStickX, 0.75);
    sim.set_axis(second, GamepadAxis::LeftStickX, -0.5);
    sim.mouse_motion(Vec2::new(12.0, -3.0));
    sim.mouse_motion(Vec2::new(-2.0, 1.0));
    sim.tick();
    let frame = &sim.resource::<Game>().history[0];
    assert_eq!(frame.motion, Vec2::new(10.0, -2.0));
    assert!(frame.pads[0].1);
    assert!(frame.pads[0].2);
    assert!(!frame.pads[1].1);
    assert_eq!((frame.pads[0].4, frame.pads[1].4), (0.75, -0.5));
    sim.tick();
    let frame = &sim.resource::<Game>().history[1];
    assert_eq!(frame.motion, Vec2::ZERO);
    assert!(frame.pads[0].1 && !frame.pads[0].2);
    assert_eq!((frame.pads[0].4, frame.pads[1].4), (0.75, -0.5));
    sim.release(first.button(GamepadButton::South));
    sim.set_axis(first, GamepadAxis::LeftStickX, 0.0);
    sim.tick();
    let frame = &sim.resource::<Game>().history[2];
    assert!(!frame.pads[0].1 && frame.pads[0].3);
    assert_eq!(frame.pads[0].4, 0.0);
    assert_eq!(sim.resource::<Game>().jumps, 1);
    assert_eq!(sim.resource::<Game>().position, 0.0);
    assert_eq!(sim.resource::<Game>().look, Vec2::new(10.0, -2.0));
    sim.tick();
    assert!(!sim.resource::<Game>().history[3].pads[0].3);
}

#[derive(Resource, Default)]
struct AxisEvents(Vec<f32>);

#[test]
fn axis_dead_zones_thresholds_and_analog_button_edges_are_filtered_by_bevy() {
    let mut sim = Sim::new(|app| {
        app.init_resource::<Game>()
            .init_resource::<AxisEvents>()
            .add_systems(
                Update,
                (
                    play,
                    |mut events: MessageReader<GamepadAxisChangedEvent>,
                     mut values: ResMut<AxisEvents>| {
                        values.0.extend(events.read().map(|event| event.value));
                    },
                ),
            );
    });
    let slot = sim.connect_gamepad();
    let entity = sim.gamepad_entity(slot).unwrap();
    let mut settings = GamepadSettings::default();
    settings.default_axis_settings.set_deadzone_lowerbound(-0.2);
    settings.default_axis_settings.set_deadzone_upperbound(0.2);
    settings.default_axis_settings.set_threshold(0.1);
    sim.world_mut().entity_mut(entity).insert(settings);
    sim.set_axis(slot, GamepadAxis::LeftStickX, 0.15);
    sim.set_button_value(slot, GamepadButton::South, 0.8);
    sim.tick();
    assert_eq!(sim.resource::<Game>().history[0].pads[0].4, 0.0);
    assert!(sim.resource::<Game>().history[0].pads[0].2);
    assert_eq!(
        sim.world()
            .get::<Gamepad>(entity)
            .unwrap()
            .get(GamepadButton::South),
        Some(0.8)
    );
    sim.set_axis(slot, GamepadAxis::LeftStickX, 0.5);
    sim.tick();
    sim.set_axis(slot, GamepadAxis::LeftStickX, 0.55);
    sim.set_button_value(slot, GamepadButton::South, 0.2);
    sim.tick();
    assert_eq!(sim.resource::<Game>().history[2].pads[0].4, 0.5);
    assert!(sim.resource::<Game>().history[2].pads[0].3);
    sim.set_axis(slot, GamepadAxis::LeftStickX, -0.1);
    sim.tick();
    // Bevy 0.20 stores accepted raw values in Gamepad; processed events carry
    // the scaled, dead-zone-aware value. Do not patch either in the harness.
    assert_eq!(sim.resource::<Game>().history[3].pads[0].4, -0.1);
    assert_eq!(sim.resource::<AxisEvents>().0, vec![0.375, 0.0]);
}

fn script() -> InputScript {
    InputScript::from_ron(
        r#"(
        version: 2,
        events: [
            (tick: 0, action: ConnectGamepad(slot: 0)),
            (tick: 0, action: Press(Gamepad(slot: 0, button: South))),
            (tick: 0, action: SetAxis(slot: 0, axis: LeftStickX, value: 0.75)),
            (tick: 0, action: SetAxis(slot: 7, axis: LeftStickX, value: -0.5)),
            (tick: 0, action: MouseMotion(x: 12.0, y: -3.0)),
            (tick: 0, action: MouseMotion(x: -2.0, y: 1.0)),
            (tick: 2, action: Release(Gamepad(slot: 0, button: South))),
            (tick: 2, action: Tap(Gamepad(slot: 7, button: South))),
            (tick: 2, action: SetButtonValue(slot: 0, button: South, value: 0.8)),
            (tick: 3, action: Press(Gamepad(slot: 7, button: South))),
            (tick: 3, action: ConnectGamepad(slot: 0)),
            (tick: 4, action: SetAxis(slot: 0, axis: LeftStickX, value: 0.0)),
        ],
    )"#,
    )
    .unwrap()
}

#[test]
fn version_two_roundtrips_and_matches_helpers_and_split_playback() {
    assert_eq!(InputScript::default().version, SCRIPT_VERSION);
    let script = script();
    let decoded = InputScript::from_ron(&script.to_ron().unwrap()).unwrap();
    assert_eq!(script, decoded);
    let mut helpers = sim();
    let first = helpers.connect_gamepad();
    let other = GamepadSlot(7);
    helpers.press(first.button(GamepadButton::South));
    helpers.set_axis(first, GamepadAxis::LeftStickX, 0.75);
    helpers.set_axis(other, GamepadAxis::LeftStickX, -0.5);
    helpers.mouse_motion(Vec2::new(12.0, -3.0));
    helpers.mouse_motion(Vec2::new(-2.0, 1.0));
    helpers.run_ticks(2);
    helpers.release(first.button(GamepadButton::South));
    helpers.set_button_value(first, GamepadButton::South, 0.8);
    helpers.tap(other.button(GamepadButton::South));
    helpers.press(other.button(GamepadButton::South));
    helpers.tick();
    helpers.set_axis(first, GamepadAxis::LeftStickX, 0.0);
    helpers.run_ticks(2);
    let mut playback = sim();
    playback.run_script(&decoded, 6);
    assert_eq!(
        helpers.resource::<Game>().history,
        playback.resource::<Game>().history
    );
    let mut split = sim();
    split.run_script(&decoded, 3);
    let entity = split.gamepad_entity(first).unwrap();
    split.run_script(&decoded, 6);
    assert_eq!(Some(entity), split.gamepad_entity(first));
    assert_eq!(
        split.resource::<Game>().history,
        playback.resource::<Game>().history
    );
    assert_eq!(
        playback.resource::<Game>().history[3].pads[0].4,
        0.75,
        "reconnecting is a no-op"
    );
    assert!(
        playback.resource::<Game>().history[3].pads[1].2,
        "tap release precedes next scripted press"
    );
    assert_eq!(
        split.connect_gamepad(),
        GamepadSlot(1),
        "sparse slots do not allocate intermediate gamepads"
    );
}

#[test]
fn endpoint_gamepad_tap_releases_with_an_ordinary_tick() {
    let mut sim = sim();
    let script = InputScript {
        version: 2,
        events: vec![ScriptEvent {
            tick: 0,
            action: InputAction::Tap(GamepadSlot(42).button(GamepadButton::South)),
        }],
    };
    sim.run_script(&script, 1);
    assert!(sim.resource::<Game>().history[0].pads[0].2);
    sim.tick();
    assert!(sim.resource::<Game>().history[1].pads[0].3);
    sim.tick();
    assert!(!sim.resource::<Game>().history[2].pads[0].3);
}

#[test]
fn invalid_actions_fail_before_playback_mutates_the_world() {
    let invalid = [
        InputAction::SetAxis {
            slot: GamepadSlot(0),
            axis: GamepadAxis::LeftStickX,
            value: f32::NAN,
        },
        InputAction::SetAxis {
            slot: GamepadSlot(0),
            axis: GamepadAxis::LeftStickX,
            value: 1.01,
        },
        InputAction::SetButtonValue {
            slot: GamepadSlot(0),
            button: GamepadButton::South,
            value: -0.1,
        },
        InputAction::MouseMotion {
            x: 0.0,
            y: f32::INFINITY,
        },
    ];
    for action in invalid {
        let mut sim = sim();
        let script = InputScript {
            version: 2,
            events: vec![
                ScriptEvent {
                    tick: 0,
                    action: InputAction::ConnectGamepad {
                        slot: GamepadSlot(0),
                    },
                },
                ScriptEvent { tick: 1, action },
            ],
        };
        assert!(catch_unwind(|| script.validate()).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| sim.run_script(&script, 2))).is_err());
        assert_eq!(sim.current_tick(), 0);
        assert_eq!(sim.gamepad_entity(GamepadSlot(0)), None);
    }
    for event in script().events {
        let mut sim = sim();
        let script = InputScript {
            version: 1,
            events: vec![event],
        };
        assert!(catch_unwind(|| script.validate()).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| sim.run_script(&script, 10))).is_err());
        assert_eq!(sim.current_tick(), 0);
    }
}

#[derive(Resource, Default, Debug, PartialEq)]
struct RawInputs {
    combined: usize,
    connections: usize,
    axes: Vec<f32>,
    buttons: Vec<f32>,
}

fn raw_sim() -> Sim {
    Sim::new(|app| {
        app.init_resource::<RawInputs>().add_systems(
            Update,
            |mut combined: MessageReader<RawGamepadEvent>,
             mut connections: MessageReader<GamepadConnectionEvent>,
             mut axes: MessageReader<RawGamepadAxisChangedEvent>,
             mut buttons: MessageReader<RawGamepadButtonChangedEvent>,
             mut inputs: ResMut<RawInputs>| {
                inputs.combined += combined.read().count();
                inputs.connections += connections.read().count();
                inputs.axes.extend(axes.read().map(|event| event.value));
                inputs
                    .buttons
                    .extend(buttons.read().map(|event| event.value));
            },
        );
    })
}

#[test]
fn helpers_and_scripts_publish_combined_and_typed_raw_gamepad_messages() {
    let mut helpers = raw_sim();
    let pad = helpers.connect_gamepad();
    helpers.press(pad.button(GamepadButton::South));
    helpers.set_axis(pad, GamepadAxis::LeftStickX, 0.75);
    helpers.tick();
    helpers.release(pad.button(GamepadButton::South));
    helpers.tick();
    let mut playback = raw_sim();
    let script = InputScript::from_ron(
        "(version:2,events:[
        (tick:0,action:Press(Gamepad(slot:0,button:South))),
        (tick:0,action:SetAxis(slot:0,axis:LeftStickX,value:0.75)),
        (tick:1,action:Release(Gamepad(slot:0,button:South)))])",
    )
    .unwrap();
    playback.run_script(&script, 2);
    assert_eq!(
        helpers.resource::<RawInputs>(),
        playback.resource::<RawInputs>()
    );
    assert_eq!(
        helpers.resource::<RawInputs>(),
        &RawInputs {
            combined: 4,
            connections: 1,
            axes: vec![0.75],
            buttons: vec![1.0, 0.0],
        }
    );
}

#[test]
fn custom_button_thresholds_preserve_digital_state_between_press_and_release() {
    let mut sim = sim();
    let pad = sim.connect_gamepad();
    let entity = sim.gamepad_entity(pad).unwrap();
    let settings = GamepadSettings {
        default_button_settings: ButtonSettings::new(0.9, 0.1).unwrap(),
        ..Default::default()
    };
    sim.world_mut().entity_mut(entity).insert(settings);
    for value in [0.8, 1.0, 0.5, 0.0, 0.5] {
        sim.set_button_value(pad, GamepadButton::South, value);
        sim.tick();
    }
    let history = &sim.resource::<Game>().history;
    assert!(!history[0].pads[0].1);
    assert!(history[1].pads[0].2);
    assert!(history[2].pads[0].1 && !history[2].pads[0].2);
    assert!(history[3].pads[0].3);
    assert!(!history[4].pads[0].1 && !history[4].pads[0].3);
}

#[test]
fn unknown_fields_inside_actions_and_gamepad_buttons_are_rejected() {
    for action in [
        "SetAxis(slot:0,axis:LeftStickX,value:0.5,typo:1)",
        "SetButtonValue(slot:0,button:South,value:0.8,typo:1)",
        "MouseMotion(x:1.0,y:0.0,typo:1)",
        "ConnectGamepad(slot:0,typo:1)",
        "Press(Gamepad(slot:0,button:South,typo:1))",
    ] {
        let source = format!("(version:2,events:[(tick:0,action:{action})])");
        assert!(InputScript::from_ron(&source).is_err(), "accepted {action}");
    }
}

#[test]
fn invalid_helper_values_are_not_queued() {
    let mut sim = sim();
    assert!(catch_unwind(AssertUnwindSafe(|| sim.set_axis(
        GamepadSlot(0),
        GamepadAxis::LeftStickX,
        f32::NEG_INFINITY
    )))
    .is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| sim.set_button_value(
        GamepadSlot(0),
        GamepadButton::South,
        2.0
    )))
    .is_err());
    assert!(catch_unwind(AssertUnwindSafe(
        || sim.mouse_motion(Vec2::new(f32::NAN, 0.0))
    ))
    .is_err());
    sim.tick();
    assert!(sim.resource::<Game>().history[0].pads.is_empty());
    assert_eq!(sim.resource::<Game>().look, Vec2::ZERO);
}
