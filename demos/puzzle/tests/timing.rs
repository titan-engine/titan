//! Frame/fixed timing and self-contained events, using Titan's headless harness.

use bevy::prelude::*;
use titan_puzzle::{
    content::starter_levels,
    simulation::{Direction, EventKind},
    Action, Game, GameplayActions, GameplayEvent, GameplayPlugin, FIXED_HZ,
};
use titan_test::Sim;

#[derive(Resource, Default)]
struct Recording(Vec<GameplayEvent>);

fn record(mut events: MessageReader<GameplayEvent>, mut recording: ResMut<Recording>) {
    recording.0.extend(events.read().cloned());
}

#[test]
fn zero_and_multiple_fixed_ticks_preserve_fifo_and_reset_snapshots() {
    let mut sim = Sim::new(|app| {
        app.insert_resource(Game::new(starter_levels()).unwrap())
            .init_resource::<Recording>()
            .add_plugins(GameplayPlugin)
            .add_systems(Update, record);
    })
    .with_fixed_dt(1.0 / 60.0);
    sim.world_mut()
        .insert_resource(Time::<Fixed>::from_hz(FIXED_HZ));
    sim.world_mut()
        .resource_mut::<GameplayActions>()
        .0
        .push_back(Action::Move(Direction::Right));
    sim.run_ticks(1);
    assert_eq!(sim.world().resource::<Game>().sequence(), 0);
    assert_eq!(sim.world().resource::<GameplayActions>().0.len(), 1);
    sim.run_ticks(4);
    let solved = sim.world().resource::<Game>().state().clone();
    assert!(solved.complete());

    // Several actions happen before Update readers run. Restoration must carry
    // its own exact state, not ask readers to consult the latest Game resource.
    sim.world_mut()
        .resource_mut::<GameplayActions>()
        .0
        .extend([Action::Undo, Action::Redo]);
    sim.world_mut()
        .insert_resource(Time::<Fixed>::from_seconds(1.0 / 120.0));
    sim.run_ticks(1);
    assert_eq!(*sim.world().resource::<Game>().state(), solved);
    let recording = &sim.world().resource::<Recording>().0;
    let restores: Vec<_> = recording
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::Restored { action, state } => Some((*action, state)),
            _ => None,
        })
        .collect();
    assert_eq!(restores.len(), 2);
    assert_eq!(restores[0].0, Action::Undo);
    assert!(!restores[0].1.complete());
    assert_eq!(restores[0].1.moves(), 0);
    assert_eq!(restores[1].0, Action::Redo);
    assert_eq!(*restores[1].1, solved);

    sim.world_mut()
        .resource_mut::<GameplayActions>()
        .0
        .extend([Action::NextLevel, Action::Move(Direction::Right)]);
    sim.run_ticks(1);
    let recording = &sim.world().resource::<Recording>().0;
    let (level, state) = recording
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::LevelStarted { level, state } => Some((level, state)),
            _ => None,
        })
        .unwrap();
    let game = sim.world().resource::<Game>();
    assert_eq!(level.id(), game.level().id());
    assert_eq!(state.player(), level.player().position);
    assert_eq!(state.moves(), 0);
    assert_eq!(game.state().moves(), 1);
    assert_ne!(state.player(), game.state().player());
}
