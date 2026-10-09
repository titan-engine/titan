//! Keyboard and mouse button transitions observed through real input messages.

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{
    keyboard::{KeyCode, KeyboardInput},
    mouse::{MouseButton, MouseButtonInput},
    ButtonInput, ButtonState,
};
use titan_test::Sim;

#[derive(Debug, Default, PartialEq, Eq)]
struct ButtonSnapshot {
    pressed: bool,
    just_pressed: bool,
    just_released: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Frame {
    key: ButtonSnapshot,
    mouse: ButtonSnapshot,
    keyboard_messages: Vec<(KeyCode, ButtonState)>,
    mouse_messages: Vec<(MouseButton, ButtonState)>,
}

#[derive(Resource, Default)]
struct Frames(Vec<Frame>);

fn record_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard_messages: MessageReader<KeyboardInput>,
    mut mouse_messages: MessageReader<MouseButtonInput>,
    mut frames: ResMut<Frames>,
) {
    frames.0.push(Frame {
        key: ButtonSnapshot {
            pressed: keys.pressed(KeyCode::Space),
            just_pressed: keys.just_pressed(KeyCode::Space),
            just_released: keys.just_released(KeyCode::Space),
        },
        mouse: ButtonSnapshot {
            pressed: mouse.pressed(MouseButton::Left),
            just_pressed: mouse.just_pressed(MouseButton::Left),
            just_released: mouse.just_released(MouseButton::Left),
        },
        keyboard_messages: keyboard_messages
            .read()
            .map(|message| (message.key_code, message.state))
            .collect(),
        mouse_messages: mouse_messages
            .read()
            .map(|message| (message.button, message.state))
            .collect(),
    });
}

fn sim() -> Sim {
    Sim::new(|app| {
        app.init_resource::<Frames>()
            .add_systems(Update, record_input);
    })
}

fn pressed() -> ButtonSnapshot {
    ButtonSnapshot {
        pressed: true,
        just_pressed: true,
        just_released: false,
    }
}

fn held() -> ButtonSnapshot {
    ButtonSnapshot {
        pressed: true,
        ..Default::default()
    }
}

fn released() -> ButtonSnapshot {
    ButtonSnapshot {
        just_released: true,
        ..Default::default()
    }
}

#[test]
fn press_and_release_are_queued_and_emit_real_input_messages() {
    let mut sim = sim();
    sim.press(KeyCode::Space);
    sim.press(MouseButton::Left);
    assert_eq!(sim.current_tick(), 0);
    assert!(sim.resource::<Frames>().0.is_empty());

    sim.tick();
    sim.tick();
    sim.release(KeyCode::Space);
    sim.release(MouseButton::Left);
    assert_eq!(sim.current_tick(), 2);
    sim.tick();
    sim.tick();

    assert_eq!(
        sim.resource::<Frames>().0,
        vec![
            Frame {
                key: pressed(),
                mouse: pressed(),
                keyboard_messages: vec![(KeyCode::Space, ButtonState::Pressed)],
                mouse_messages: vec![(MouseButton::Left, ButtonState::Pressed)],
            },
            Frame {
                key: held(),
                mouse: held(),
                ..Default::default()
            },
            Frame {
                key: released(),
                mouse: released(),
                keyboard_messages: vec![(KeyCode::Space, ButtonState::Released)],
                mouse_messages: vec![(MouseButton::Left, ButtonState::Released)],
            },
            Frame::default(),
        ]
    );
}

#[test]
fn keyboard_tap_advances_once_and_releases_on_the_next_tick() {
    let mut sim = sim();
    sim.tap(KeyCode::Space);
    assert_eq!(sim.current_tick(), 1);
    assert_eq!(sim.resource::<Frames>().0[0].key, pressed());
    assert_eq!(
        sim.resource::<Frames>().0[0].keyboard_messages,
        vec![(KeyCode::Space, ButtonState::Pressed)]
    );

    sim.tick();
    assert_eq!(sim.resource::<Frames>().0[1].key, released());
    assert_eq!(
        sim.resource::<Frames>().0[1].keyboard_messages,
        vec![(KeyCode::Space, ButtonState::Released)]
    );
    sim.tick();
    assert_eq!(sim.resource::<Frames>().0[2], Frame::default());
}

#[test]
fn mouse_tap_advances_once_and_releases_on_the_next_tick() {
    let mut sim = sim();
    sim.tap(MouseButton::Left);
    assert_eq!(sim.current_tick(), 1);
    assert_eq!(sim.resource::<Frames>().0[0].mouse, pressed());
    assert_eq!(
        sim.resource::<Frames>().0[0].mouse_messages,
        vec![(MouseButton::Left, ButtonState::Pressed)]
    );

    sim.tick();
    assert_eq!(sim.resource::<Frames>().0[1].mouse, released());
    assert_eq!(
        sim.resource::<Frames>().0[1].mouse_messages,
        vec![(MouseButton::Left, ButtonState::Released)]
    );
    sim.tick();
    assert_eq!(sim.resource::<Frames>().0[2], Frame::default());
}
