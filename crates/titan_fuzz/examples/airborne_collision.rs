//! Find and replay a deliberate collision bug without a window or renderer.
//! Run: `cargo run -p titan_fuzz --example airborne_collision`

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, ButtonInput};
use titan_fuzz::{Fuzz, FuzzReport, Generator};
use titan_test::Sim;

#[derive(Resource, Default)]
struct Player {
    x: i32,
    airborne_ticks: u8,
}

fn move_player(keys: Res<ButtonInput<KeyCode>>, mut player: ResMut<Player>) {
    if keys.just_pressed(KeyCode::Space) {
        player.airborne_ticks = 6;
    }
    if keys.pressed(KeyCode::KeyD) {
        player.x += 1;
    }
    // BUG: the wall at x=2 should apply while airborne as well as grounded.
    if player.airborne_ticks == 0 {
        player.x = player.x.min(2);
    }
    player.airborne_ticks = player.airborne_ticks.saturating_sub(1);
}

fn game() -> Sim {
    Sim::new(|app| {
        app.init_resource::<Player>()
            .add_systems(Update, move_player);
    })
    .with_seed(7)
}

fn inside_wall(world: &World) -> Result<(), String> {
    let x = world.resource::<Player>().x;
    if x <= 2 {
        Ok(())
    } else {
        Err(format!("player crossed wall: x={x}"))
    }
}

#[expect(
    clippy::print_stdout,
    reason = "This example demonstrates the failure report"
)]
fn main() {
    let report = Fuzz::new(game)
        .buttons([KeyCode::KeyD, KeyCode::Space])
        .invariant("inside wall", inside_wall)
        .no_nan_transforms()
        .generator(Generator {
            event_density: 0.35,
            min_hold_ticks: 1,
            max_hold_ticks: 10,
        })
        .cases(8)
        .ticks(128)
        .seed(1)
        .max_shrink_runs(300)
        .test_name("airborne_collision")
        .run();

    println!("{report}");
    let FuzzReport::Failed(failure) = report else {
        panic!("expected the deliberately broken game to fail reproducibly");
    };
    assert_eq!(failure.invariant, "inside wall");

    // A finding is an ordinary titan_test script: no fuzzer needed to replay.
    let mut replay = game();
    replay.run_script(&failure.script, failure.ticks);
    assert!(inside_wall(replay.world()).is_err());
    println!(
        "Plain Sim replay confirmed the expected bug. Fix the collision guard to make it pass."
    );
    // We intentionally do not call assert_ok(): finding this bug is success for
    // this demonstration. In real tests, assert_ok() saves the script and fails.
}
