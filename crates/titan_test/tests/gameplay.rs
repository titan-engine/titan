//! A complete first gameplay test. Run with `cargo test -p titan_test --test gameplay`.

use bevy_app::{App, FixedUpdate, Plugin, Startup};
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, ButtonInput};
use bevy_time::{Fixed, Time};
use titan_test::Sim;

#[derive(Component, Debug)]
struct Jumper {
    height: f32,
    velocity: f32,
    grounded: bool,
    jumps: u32,
    landings: u32,
}

struct JumpGamePlugin;

impl Plugin for JumpGamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .add_systems(FixedUpdate, jump_and_fall);
    }
}

fn spawn_player(mut commands: Commands) {
    commands.spawn(Jumper {
        height: 0.0,
        velocity: 0.0,
        grounded: true,
        jumps: 0,
        landings: 0,
    });
}

fn jump_and_fall(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Fixed>>,
    mut players: Query<&mut Jumper>,
) {
    for mut player in &mut players {
        if input.just_pressed(KeyCode::Space) && player.grounded {
            player.velocity = 5.0;
            player.grounded = false;
            player.jumps += 1;
        }
        if !player.grounded {
            player.velocity -= 10.0 * time.delta_secs();
            player.height += player.velocity * time.delta_secs();
            if player.height <= 0.0 {
                player.height = 0.0;
                player.velocity = 0.0;
                player.grounded = true;
                player.landings += 1;
            }
        }
    }
}

#[test]
fn player_jumps_and_lands_after_a_space_tap() {
    let mut sim = Sim::new(|app| {
        app.add_plugins(JumpGamePlugin);
    })
    .with_fixed_dt(1.0 / 60.0);

    sim.tick(); // Startup spawns the player. No window, renderer, or sleep is needed.
    assert!(sim.single::<&Jumper, ()>().grounded);

    sim.tap(KeyCode::Space); // Press and advance one tick; release on the next tick.
    let player = sim.single::<&Jumper, ()>();
    assert!(!player.grounded);
    assert!(player.height > 0.0);
    assert_eq!(player.jumps, 1);

    let ticks_to_land = sim.run_until_named(120, "player lands after jumping", |world| {
        world
            .iter_entities()
            .filter_map(|entity| entity.get::<Jumper>())
            .any(|player| player.landings == 1 && player.grounded)
    });
    assert!(ticks_to_land > 0 && ticks_to_land < 120);
    let player = sim.single::<&Jumper, ()>();
    assert_eq!(player.height, 0.0);
    assert_eq!(player.velocity, 0.0);
    assert_eq!(player.jumps, 1);
    assert_eq!(player.landings, 1);

    sim.run_ticks(10);
    assert_eq!(
        sim.single::<&Jumper, ()>().jumps,
        1,
        "tap must not leave Space held"
    );
}
