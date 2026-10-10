//! Push-puzzle foundation: validated content, deterministic rules, and an ECS adapter.
//! See the demo README for the action/event contract and extension ownership.

pub mod content;
pub mod level;
pub mod simulation;

extern crate alloc;

use alloc::collections::VecDeque;

use bevy::prelude::*;
pub use simulation::{Action, Game, GameplayEvent};

/// Fixed gameplay rate. Each tick consumes at most one queued discrete action.
pub const FIXED_HZ: f64 = 12.0;

/// FIFO shared by human input and scripts. Empty ticks never mutate the game.
#[derive(Resource, Default, Debug)]
pub struct GameplayActions(pub VecDeque<Action>);

/// Scheduling boundary for systems that need to run before/after gameplay.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Simulation;

/// Install after inserting [`Game`]; no renderer or input plugins are required.
/// Messages have standard Bevy retention: consume every update, or record them.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameplayActions>()
            .add_message::<GameplayEvent>()
            .add_systems(FixedUpdate, process_action.in_set(Simulation));
    }
}

fn process_action(
    mut actions: ResMut<GameplayActions>,
    mut game: ResMut<Game>,
    mut events: MessageWriter<GameplayEvent>,
) {
    if let Some(action) = actions.0.pop_front() {
        events.write_batch(game.step(action));
    }
}
