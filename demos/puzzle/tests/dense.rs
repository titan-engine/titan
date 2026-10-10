//! Dense valid content exercises occupancy lookup and atomic history restoration.

use core::fmt::Write;
use titan_puzzle::{
    level::{Cell, Level},
    simulation::{BlockReason, Direction, EventKind},
    Action, Game,
};

#[test]
fn thousands_of_blocks_keep_reverse_occupancy_consistent_across_actions() {
    let mut source = String::from("puzzle 1\nid dense\ntitle Dense Occupancy\ngrid\n");
    writeln!(source, "{}", "#".repeat(64)).unwrap();
    for _ in 0..62 {
        writeln!(source, "#{}#", ".".repeat(62)).unwrap();
    }
    writeln!(
        source,
        "{}\nobjects\nplayer hero 1 1\nblock moving 3 1\ntarget destination 4 1",
        "#".repeat(64)
    )
    .unwrap();
    for y in 1..63 {
        for x in 1..63 {
            if y == 1 && x <= 4 {
                continue;
            }
            writeln!(source, "block b-{x}-{y} {x} {y}\ntarget t-{x}-{y} {x} {y}").unwrap();
        }
    }
    source.push_str("end\n");
    let mut game = Game::new(vec![Level::parse("dense.puzzle", &source).unwrap()]).unwrap();
    let start = game.state().clone();
    assert!(start.blocks().len() > 3000);
    let events = game.step(Action::Move(Direction::Up));
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].kind,
        EventKind::Blocked {
            direction: Direction::Up,
            reason: BlockReason::Wall
        }
    );
    assert_eq!(*game.state(), start);
    assert_eq!(game.step(Action::Undo).len(), 1);
    game.step(Action::Move(Direction::Right));
    let before_push = game.state().clone();
    let events = game.step(Action::Move(Direction::Right));
    assert!(game.state().complete());
    assert_eq!(game.state().block_at(Cell::new(3, 1)), None);
    assert_eq!(game.state().block_at(Cell::new(4, 1)), Some("moving"));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::TargetChanged { .. }))
            .count(),
        1
    );
    assert_eq!(events.last().unwrap().kind, EventKind::LevelComplete);
    let solved = game.state().clone();
    for action in [Action::Undo, Action::Redo, Action::Restart, Action::Undo] {
        game.step(action);
        for (id, cell) in game.state().blocks() {
            assert_eq!(game.state().block_at(*cell), Some(id.as_str()));
        }
        assert_eq!(game.state().block_at(game.state().player()), None);
    }
    assert_eq!(*game.state(), solved);
    game.step(Action::Undo);
    // Restart's undo restores the solved board, then the next undo undoes its
    // winning push; reverse occupancy must match that exact earlier snapshot.
    assert_eq!(*game.state(), before_push);
    assert_eq!(game.state().block_at(Cell::new(3, 1)), Some("moving"));
    assert_eq!(game.state().block_at(Cell::new(4, 1)), None);
}
