//! Headless acceptance scenarios exercising the same actions as human input.

use bevy::{ecs::message::MessageCursor, prelude::*};
use titan_puzzle::{
    content::{starter_levels, SOLUTIONS},
    level::{Cell, Level},
    simulation::{BlockReason, Direction, EventKind, IgnoreReason},
    Action, Game, GameplayActions, GameplayEvent, GameplayPlugin,
};

fn level(objects: &str) -> Level {
    Level::parse("rules.puzzle", &format!("puzzle 1\nid rules\ntitle Rules\ngrid\n########\n#......#\n#......#\n#......#\n#......#\n########\nobjects\n{objects}\nend\n")).unwrap()
}

fn game() -> Game {
    Game::new(vec![level(
        "player hero 1 2\nblock a 2 2\nblock b 2 3\ntarget ta 4 2\ntarget tb 4 3",
    )])
    .unwrap()
}

fn right(game: &mut Game) -> Vec<GameplayEvent> {
    game.step(Action::Move(Direction::Right))
}

#[test]
fn walk_push_target_cover_and_uncover_are_ordered_facts() {
    let mut game = game();
    let events = right(&mut game);
    assert!(
        matches!(&events[0].kind, EventKind::Pushed { id, from, to } if id == "a" && *from == Cell::new(2,2) && *to == Cell::new(3,2))
    );
    assert!(matches!(events[1].kind, EventKind::Moved { .. }));
    assert_eq!(game.state().moves(), 1);
    let events = right(&mut game);
    assert_eq!(
        events[2].kind,
        EventKind::TargetChanged {
            id: "ta".into(),
            block: Some("a".into())
        }
    );
    assert!(!game.state().complete());
    let events = right(&mut game);
    assert_eq!(
        events[2].kind,
        EventKind::TargetChanged {
            id: "ta".into(),
            block: None
        }
    );
    game.step(Action::Move(Direction::Up));
    assert_eq!(game.state().player(), Cell::new(4, 1));
    assert_eq!(game.state().moves(), 4);
}

#[test]
fn walls_and_other_blocks_reject_atomically_without_history() {
    for (objects, reason) in [
        (
            "player hero 1 1\nblock a 2 2\ntarget t 4 2",
            BlockReason::Wall,
        ),
        (
            "player hero 5 2\nblock a 6 2\ntarget t 4 2",
            BlockReason::Wall,
        ),
        (
            "player hero 1 2\nblock a 2 2\nblock b 3 2\ntarget t 4 2\ntarget u 5 2",
            BlockReason::Block,
        ),
    ] {
        let mut game = Game::new(vec![level(objects)]).unwrap();
        let before = game.state().clone();
        let direction = if game.state().player().y == 1 {
            Direction::Up
        } else {
            Direction::Right
        };
        let events = game.step(Action::Move(direction));
        assert_eq!(events[0].kind, EventKind::Blocked { direction, reason });
        assert_eq!(events.len(), 1);
        assert_eq!(*game.state(), before);
        assert!(!game.can_undo());
    }
}

#[test]
fn undo_redo_push_completion_and_restart_restore_exact_snapshots() {
    let mut game = Game::new(vec![level("player hero 1 2\nblock a 2 2\ntarget t 3 2")]).unwrap();
    let start = game.state().clone();
    let events = right(&mut game);
    assert_eq!(events.last().unwrap().kind, EventKind::LevelComplete);
    let solved = game.state().clone();
    assert!(solved.complete());
    assert_eq!(
        right(&mut game)[0].kind,
        EventKind::Blocked {
            direction: Direction::Right,
            reason: BlockReason::Complete
        }
    );
    let events = game.step(Action::Undo);
    assert_eq!(*game.state(), start);
    assert_eq!(
        events[0].kind,
        EventKind::Restored {
            action: Action::Undo,
            state: start.clone()
        }
    );
    assert_eq!(events.last().unwrap().kind, EventKind::LevelReopened);
    let events = game.step(Action::Redo);
    assert_eq!(*game.state(), solved);
    assert_eq!(events.last().unwrap().kind, EventKind::LevelComplete);
    game.step(Action::Restart);
    assert_eq!(*game.state(), start);
    game.step(Action::Undo);
    assert_eq!(*game.state(), solved);
    game.step(Action::Redo);
    assert_eq!(*game.state(), start);
}

#[test]
fn new_mutations_clear_redo_but_blocked_and_noop_actions_do_not() {
    let mut game = game();
    assert_eq!(
        game.step(Action::Undo)[0].kind,
        EventKind::Ignored(IgnoreReason::NoUndo)
    );
    assert_eq!(
        game.step(Action::Redo)[0].kind,
        EventKind::Ignored(IgnoreReason::NoRedo)
    );
    assert_eq!(
        game.step(Action::Restart)[0].kind,
        EventKind::Ignored(IgnoreReason::AtStart)
    );
    assert_eq!(
        game.step(Action::NextLevel)[0].kind,
        EventKind::Ignored(IgnoreReason::NotComplete)
    );
    right(&mut game);
    game.step(Action::Undo);
    game.step(Action::Move(Direction::Left));
    game.step(Action::Restart);
    assert!(game.can_redo());
    game.step(Action::Move(Direction::Up));
    assert!(!game.can_redo());
    assert_eq!(game.state().moves(), 1);
    game.step(Action::Restart);
    assert_eq!(game.state().moves(), 0);
    game.step(Action::Undo);
    assert_eq!(game.state().moves(), 1);
}

fn direction(c: char) -> Direction {
    match c {
        'U' => Direction::Up,
        'D' => Direction::Down,
        'L' => Direction::Left,
        'R' => Direction::Right,
        _ => panic!("invalid solution direction"),
    }
}

#[test]
fn every_starter_solution_uses_shared_queue_and_advances_campaign() {
    let mut app = App::new();
    app.insert_resource(Game::new(starter_levels()).unwrap())
        .add_plugins(GameplayPlugin);
    let mut visual_reader = MessageCursor::<GameplayEvent>::default();
    let mut audio_reader = MessageCursor::<GameplayEvent>::default();
    let mut all_events = Vec::new();
    for (index, solution) in SOLUTIONS.iter().enumerate() {
        assert_eq!(app.world().resource::<Game>().level_index(), index);
        for c in solution.chars() {
            app.world_mut()
                .resource_mut::<GameplayActions>()
                .0
                .push_back(Action::Move(direction(c)));
            app.world_mut().run_schedule(FixedUpdate);
            assert!(app.world().resource::<GameplayActions>().0.is_empty());
        }
        assert!(
            app.world().resource::<Game>().state().complete(),
            "starter {index}"
        );
        let messages = app.world().resource::<Messages<GameplayEvent>>();
        let visual: Vec<_> = visual_reader.read(messages).cloned().collect();
        let audio: Vec<_> = audio_reader.read(messages).cloned().collect();
        assert_eq!(visual, audio);
        assert_eq!(
            visual
                .iter()
                .filter(|e| e.kind == EventKind::LevelComplete)
                .count(),
            1
        );
        all_events.extend(visual);
        app.world_mut()
            .resource_mut::<GameplayActions>()
            .0
            .push_back(Action::NextLevel);
        app.world_mut().run_schedule(FixedUpdate);
        let game = app.world().resource::<Game>();
        if index + 1 < SOLUTIONS.len() {
            assert_eq!(game.level_index(), index + 1);
            assert_eq!(game.state().moves(), 0);
            assert!(!game.can_undo());
            assert!(!game.can_redo());
        } else {
            assert!(game.state().complete());
        }
    }
    assert!(all_events
        .windows(2)
        .all(|pair| pair[0].sequence <= pair[1].sequence));
}

#[test]
fn empty_ticks_and_multiple_queued_actions_have_explicit_semantics() {
    let mut app = App::new();
    app.insert_resource(game()).add_plugins(GameplayPlugin);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Game>().sequence(), 0);
    app.world_mut()
        .resource_mut::<GameplayActions>()
        .0
        .extend([Action::Move(Direction::Right), Action::Undo]);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Game>().state().moves(), 1);
    assert_eq!(app.world().resource::<GameplayActions>().0.len(), 1);
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Game>().state().moves(), 0);
}

#[test]
fn replay_is_exact_and_unrelated_entity_allocations_do_not_change_identity() {
    let actions = [
        Action::Move(Direction::Right),
        Action::Move(Direction::Right),
        Action::Undo,
        Action::Redo,
        Action::Restart,
        Action::Undo,
    ];
    let run = |noise| {
        let mut app = App::new();
        app.insert_resource(game()).add_plugins(GameplayPlugin);
        for _ in 0..noise {
            app.world_mut().spawn_empty();
        }
        let events: Vec<_> = actions
            .iter()
            .flat_map(|action| app.world_mut().resource_mut::<Game>().step(*action))
            .collect();
        (app.world().resource::<Game>().state().clone(), events)
    };
    assert_eq!(run(0), run(100));
    assert!(Game::new(Vec::new()).is_err());
    assert!(Game::new(vec![game().level().clone(), game().level().clone()]).is_err());
}
