//! Headless Doom acceptance scenario. Only the demo's public gameplay API is used.
use bevy_math::Vec2;
use serde::{Deserialize, Serialize};
use titan_doom::{GameplayActions, GameplayPlugin, Level, PlayerState};
use titan_fuzz::ActionFuzz;
use titan_test::Sim;

// GameplayActions deliberately does not require serialization. A regression
// DTO keeps the game's public action resource independent of fuzz tooling.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Actions {
    movement: [f32; 2],
    look_delta: [f32; 2],
}

#[test]
fn doom_player_never_enters_a_wall_under_generated_gameplay_actions() {
    ActionFuzz::new(
        || {
            Sim::new(|app| {
                app.insert_resource(Level::demo())
                    .add_plugins(GameplayPlugin);
            })
        },
        Actions::default(),
        |rng| {
            // Long straight runs, diagonal movement, analog magnitudes and
            // occasional turns explore contacts rather than random raw keys.
            let axis = |value: u64| match value {
                0 => -1.0,
                1 => 0.0,
                _ => 1.0,
            };
            Actions {
                movement: [axis(rng.below(3)), 0.25 + rng.unit_f64() as f32 * 1.75],
                look_delta: if rng.below(40) == 0 {
                    [(rng.unit_f64() as f32 - 0.5) * core::f32::consts::TAU, 0.1]
                } else {
                    [0.0; 2]
                },
            }
        },
        |world, action| {
            *world.resource_mut::<GameplayActions>() = GameplayActions {
                movement: Vec2::from_array(action.movement),
                look_delta: Vec2::from_array(action.look_delta),
            };
        },
    )
    .invariant("player outside walls", |world| {
        let player = world.resource::<PlayerState>();
        let level = world.resource::<Level>();
        let cell = player.position.floor().as_ivec2();
        if !player.position.is_finite() || level.is_wall(cell.x, cell.y) {
            Err(format!("player inside wall cell at {:?}", player.position))
        } else {
            Ok(())
        }
    })
    .invariant("gameplay advances every tick", |world| {
        if world.resource::<PlayerState>().tick > 0 {
            Ok(())
        } else {
            Err("no fixed gameplay update".into())
        }
    })
    .no_nan_transforms()
    .cases(8)
    .ticks(600)
    .seed(68)
    .max_shrink_runs(300)
    .test_name("doom-actions")
    .run()
    .assert_ok();
}
