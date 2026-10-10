//! Validation and level replacement regressions for gameplay placements.

use bevy::prelude::*;
use titan_doom::{CombatState, GameplayActions, GameplayPlugin, Level, PlayerState};

const ROWS: [&str; 5] = ["#####", "#...#", "#...#", "#...#", "#####"];
const SPAWN: &str = "(id: \"spawn\", kind: Spawn, position: (1.5, 3.5), yaw: 0.0)";
const DOOR: &str = "(id: \"door\", kind: RedDoor, position: (2.5, 2.5), yaw: 0.0)";

fn source(objects: &str) -> String {
    format!("(rows: {ROWS:?}, objects: [{objects}])")
}

#[test]
fn gameplay_placement_validation_identifies_invalid_doors_and_clearance() {
    for (objects, expected) in [
        (
            format!("{SPAWN}, {}", DOOR.replace("(2.5, 2.5)", "(2.4, 2.5)")),
            "door door must be at a cell center",
        ),
        (
            format!(
                "{SPAWN}, {DOOR}, {}",
                DOOR.replace("\"door\"", "\"second\"")
            ),
            "overlaps door",
        ),
        (
            format!(
                "{SPAWN}, {DOOR}, {}",
                DOOR.replace("RedDoor", "RedKey")
                    .replace("\"door\"", "\"key\"")
            ),
            "object key overlaps door door",
        ),
        (
            format!("{}, {DOOR}", SPAWN.replace("(1.5, 3.5)", "(1.9, 2.5)")),
            "spawn lacks closed-door clearance",
        ),
        (
            format!(
                "{SPAWN}, {DOOR}, (id: \"enemy\", kind: Enemy, position: (1.9, 2.5), yaw: 0.0)"
            ),
            "enemy enemy lacks closed-door clearance",
        ),
        (
            format!("{SPAWN}, (id: \"enemy\", kind: Enemy, position: (1.1, 2.5), yaw: 0.0)"),
            "enemy enemy lacks radius clearance",
        ),
    ] {
        let error = Level::parse(&source(&objects)).unwrap_err().to_string();
        assert!(
            error.contains(expected),
            "{error:?} should identify {expected:?}"
        );
    }
    assert!(Level::parse(&source(&format!("{SPAWN}, {DOOR}"))).is_ok());
}

#[test]
fn smaller_replacement_level_does_not_alias_or_panic_and_restart_adopts_it() {
    let mut app = App::new();
    app.insert_resource(Level::demo())
        .add_plugins(GameplayPlugin);
    app.insert_resource(Level::parse(&source(SPAWN)).unwrap());
    // Replacement alone intentionally does not reset gameplay. Stale door
    // coordinates must not index outside the new grid or alias another cell.
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<PlayerState>().tick, 1);
    app.world_mut().resource_mut::<GameplayActions>().restart = true;
    app.world_mut().run_schedule(FixedUpdate);
    let player = app.world().resource::<PlayerState>();
    assert_eq!(player.position, Vec2::new(1.5, 3.5));
    assert_eq!(player.tick, 0);
    let combat = app.world().resource::<CombatState>();
    assert_eq!(combat.objects.len(), 1);
    assert_eq!(combat.objects[0].id, "spawn");
}
