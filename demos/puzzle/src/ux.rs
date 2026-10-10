//! Minimal action adapter and progression. Issue #82 owns input, flow, and saving.

use bevy::{prelude::*, window::PrimaryWindow};
use titan_puzzle::{
    simulation::{Direction, EventKind},
    Action, Game, GameplayActions, GameplayEvent,
};

pub struct UxPlugin;

impl Plugin for UxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, human_actions.after(bevy::input::InputSystems))
            .add_systems(Update, advance_completed);
    }
}

fn human_actions(
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut actions: ResMut<GameplayActions>,
) {
    // Commands are discrete, not held input. Do not discard shared commands:
    // scripts and the progression controller also own entries in this FIFO.
    if !window.focused {
        return;
    }
    // One action per key-press frame, deterministic priority; no held-key repeat.
    for (bindings, action) in [
        ([KeyCode::KeyZ, KeyCode::Backspace], Action::Undo),
        ([KeyCode::KeyY, KeyCode::KeyY], Action::Redo),
        ([KeyCode::KeyR, KeyCode::KeyR], Action::Restart),
        (
            [KeyCode::ArrowUp, KeyCode::KeyW],
            Action::Move(Direction::Up),
        ),
        (
            [KeyCode::ArrowDown, KeyCode::KeyS],
            Action::Move(Direction::Down),
        ),
        (
            [KeyCode::ArrowLeft, KeyCode::KeyA],
            Action::Move(Direction::Left),
        ),
        (
            [KeyCode::ArrowRight, KeyCode::KeyD],
            Action::Move(Direction::Right),
        ),
    ] {
        if bindings.iter().any(|key| keys.just_pressed(*key)) {
            actions.0.push_back(action);
            break;
        }
    }
}

fn advance_completed(
    time: Res<Time>,
    game: Res<Game>,
    mut events: MessageReader<GameplayEvent>,
    mut actions: ResMut<GameplayActions>,
    mut remaining: Local<Option<f32>>,
) {
    // UX consumes facts rather than interpreting input as a win. Undo/restart
    // cancel the transition. The final level stays playable via history/restart.
    let mut started = false;
    for event in events.read() {
        match event.kind {
            EventKind::LevelComplete if game.level_index() + 1 < game.level_count() => {
                *remaining = Some(1.2);
                started = true;
            }
            EventKind::LevelReopened | EventKind::LevelStarted { .. } => *remaining = None,
            _ => {}
        }
    }
    // The completion happened during this frame; its preceding delta must not
    // shorten the time for which the player actually sees the solved board.
    if started {
        return;
    }
    if let Some(seconds) = remaining.as_mut() {
        *seconds -= time.delta_secs();
        if *seconds <= 0.0 && actions.0.is_empty() {
            actions.0.push_back(Action::NextLevel);
            *remaining = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_puzzle::content::starter_levels;

    fn app() -> App {
        let mut app = App::new();
        app.insert_resource(Game::new(starter_levels()).unwrap())
            .insert_resource(Time::<()>::default())
            .init_resource::<GameplayActions>()
            .add_message::<GameplayEvent>()
            .add_systems(Update, advance_completed);
        app
    }

    fn action(app: &mut App, action: Action) {
        let events = app.world_mut().resource_mut::<Game>().step(action);
        app.world_mut()
            .resource_mut::<Messages<GameplayEvent>>()
            .write_batch(events);
    }

    fn frame(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(seconds));
        app.world_mut().run_schedule(Update);
    }

    #[test]
    fn completion_delay_starts_at_presentation_even_on_a_long_frame() {
        let mut app = app();
        action(&mut app, Action::Move(Direction::Right));
        frame(&mut app, 2.0);
        assert!(app.world().resource::<GameplayActions>().0.is_empty());
        frame(&mut app, 1.0);
        assert!(app.world().resource::<GameplayActions>().0.is_empty());
        frame(&mut app, 0.3);
        assert_eq!(
            app.world().resource::<GameplayActions>().0.front(),
            Some(&Action::NextLevel)
        );
        app.world_mut().resource_mut::<GameplayActions>().0.clear();
        frame(&mut app, 3.0);
        assert!(app.world().resource::<GameplayActions>().0.is_empty());
    }

    #[test]
    fn focus_loss_preserves_progression_and_scripted_commands() {
        let mut app = app();
        app.add_plugins(titan_puzzle::GameplayPlugin)
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(PreUpdate, human_actions);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: false,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        action(&mut app, Action::Move(Direction::Right));
        frame(&mut app, 0.1);
        frame(&mut app, 1.3);
        app.world_mut().run_schedule(PreUpdate);
        assert_eq!(
            app.world().resource::<GameplayActions>().0.front(),
            Some(&Action::NextLevel)
        );
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(app.world().resource::<Game>().level_index(), 1);
        app.world_mut()
            .resource_mut::<GameplayActions>()
            .0
            .push_back(Action::Move(Direction::Right));
        app.world_mut().run_schedule(PreUpdate);
        assert_eq!(app.world().resource::<GameplayActions>().0.len(), 1);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(app.world().resource::<Game>().state().moves(), 1);
    }

    #[test]
    fn undo_cancels_progression_and_redo_restarts_the_full_delay() {
        let mut app = app();
        action(&mut app, Action::Move(Direction::Right));
        frame(&mut app, 0.1);
        action(&mut app, Action::Undo);
        frame(&mut app, 2.0);
        assert!(app.world().resource::<GameplayActions>().0.is_empty());
        action(&mut app, Action::Redo);
        frame(&mut app, 2.0);
        assert!(app.world().resource::<GameplayActions>().0.is_empty());
        frame(&mut app, 1.3);
        assert_eq!(
            app.world().resource::<GameplayActions>().0.front(),
            Some(&Action::NextLevel)
        );
    }
}
