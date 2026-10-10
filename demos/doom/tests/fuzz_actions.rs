//! Headless Doom acceptance scenario. Only the demo's public gameplay API is used.
use bevy::prelude::{Resource, Vec2};
use serde::{Deserialize, Serialize};
use titan_doom::{
    CombatState, GamePhase, GameplayActions, GameplayPlugin, Level, ObjectKind, PlayerState,
};
use titan_fuzz::ActionFuzz;
use titan_test::Sim;

// GameplayActions deliberately does not require serialization. A regression
// DTO keeps the game's public action resource independent of fuzz tooling.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Actions {
    movement: [f32; 2],
    look_delta: [f32; 2],
    fire: bool,
    interact: bool,
    restart: bool,
}

#[derive(Resource, Default)]
struct ExpectedGameplayTick(u64);

#[test]
fn doom_player_never_enters_a_wall_under_generated_gameplay_actions() {
    ActionFuzz::new(
        || {
            Sim::new(|app| {
                app.insert_resource(Level::demo())
                    .init_resource::<ExpectedGameplayTick>()
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
                fire: rng.below(3) == 0,
                interact: rng.below(30) == 0,
                restart: rng.below(300) == 0,
            }
        },
        |world, action| {
            // Restart resets the gameplay clock; terminal phases freeze it.
            // Track the expected next tick independently of the game's update.
            let expected = if action.restart {
                0
            } else {
                world.resource::<PlayerState>().tick
                    + u64::from(world.resource::<CombatState>().phase == GamePhase::Playing)
            };
            world.resource_mut::<ExpectedGameplayTick>().0 = expected;
            let mut actions = world.resource_mut::<GameplayActions>();
            *actions = GameplayActions {
                movement: Vec2::from_array(action.movement),
                look_delta: Vec2::from_array(action.look_delta),
                ..Default::default()
            };
            actions.fire = action.fire;
            actions.interact = action.interact;
            actions.restart = action.restart;
        },
    )
    .invariant("player outside walls", |world| {
        let player = world.resource::<PlayerState>();
        let level = world.resource::<Level>();
        let cell = player.position.floor().as_ivec2();
        let closed_door = world
            .resource::<CombatState>()
            .objects
            .iter()
            .any(|object| {
                object.kind == ObjectKind::RedDoor
                    && object.active
                    && object.position.floor().as_ivec2() == cell
            });
        if !player.position.is_finite() || level.is_wall(cell.x, cell.y) || closed_door {
            Err(format!("player inside wall cell at {:?}", player.position))
        } else {
            Ok(())
        }
    })
    .invariant("gameplay advances every tick", |world| {
        if world.resource::<CombatState>().phase != GamePhase::Playing {
            return Ok(());
        }
        let gameplay_tick = world.resource::<PlayerState>().tick;
        let expected_tick = world.resource::<ExpectedGameplayTick>().0;
        if gameplay_tick == expected_tick {
            Ok(())
        } else {
            Err(format!(
                "gameplay tick {gameplay_tick} != expected tick {expected_tick}"
            ))
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
