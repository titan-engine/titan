//! A controlled demonstration of a HashMap-order gameplay bug, without a window.
//!
//! A fixed collision hasher and opposite insertion orders expose the bug without
//! depending on random hash seeds. This intentionally changes hidden setup
//! between factories; normal Repeat checks should use identical configuration.
//! HashMap does not promise any iteration order, even with a custom hasher.

use core::hash::{BuildHasherDefault, Hasher};
use std::collections::HashMap;

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use titan_determinism::{DeterminismCheck, DeterminismReport};
use titan_test::Sim;

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Enemy {
    moves: u32,
}

// Intentionally unreflected: the report should expose the gameplay consequence,
// not compare the map's hidden setup. Opaque resource internals are not observed.
#[derive(Resource)]
struct TurnOrder(HashMap<u32, Entity, BuildHasherDefault<CollisionHasher>>);

// All keys collide, making opposite insertion orders a reproducible way to
// vary iteration on the current standard library. Never use this in production:
// it is inefficient and offers no protection against adversarial keys.
#[derive(Default)]
struct CollisionHasher;

impl Hasher for CollisionHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, _bytes: &[u8]) {}
}

fn move_first_enemy(order: Res<TurnOrder>, mut enemies: Query<&mut Enemy>) {
    // BUG: an unordered collection is being used as a gameplay priority queue.
    // A real fix is an explicit stable priority (or sorting by the enemy key).
    if let Some(&entity) = order.0.values().next() {
        enemies.get_mut(entity).expect("enemy exists").moves += 1;
    }
}

fn scenario(reverse_insertion: bool) -> Sim {
    let mut sim = Sim::new(|app| {
        app.register_type::<Enemy>()
            .add_systems(Update, move_first_enemy);
    })
    .with_seed(42);

    // Always allocate in the same order: snapshot comparison matches full entity
    // IDs, not names. Only map insertion order differs between the two worlds.
    let enemies = [
        sim.world_mut()
            .spawn((Name::new("Enemy 0"), Enemy { moves: 0 }))
            .id(),
        sim.world_mut()
            .spawn((Name::new("Enemy 1"), Enemy { moves: 0 }))
            .id(),
    ];
    let mut order = HashMap::with_capacity_and_hasher(2, BuildHasherDefault::default());
    let keys = if reverse_insertion { [1, 0] } else { [0, 1] };
    for key in keys {
        order.insert(key, enemies[key as usize]);
    }
    sim.world_mut().insert_resource(TurnOrder(order));
    sim
}

#[expect(
    clippy::print_stdout,
    reason = "This headless example demonstrates the readable report and JSON"
)]
fn main() -> Result<(), serde_json::Error> {
    // Do not assume a specified std HashMap order. Check this demonstration's
    // premise explicitly so a future implementation change cannot silently pass.
    let forward = scenario(false);
    let reverse = scenario(true);
    assert_ne!(
        forward.resource::<TurnOrder>().0.keys().next(),
        reverse.resource::<TurnOrder>().0.keys().next(),
        "the controlled insertion variants must select different enemies"
    );
    drop((forward, reverse));

    let mut reverse_insertion = false;
    let report = DeterminismCheck::new(|| {
        let sim = scenario(reverse_insertion);
        reverse_insertion = !reverse_insertion;
        sim
    })
    .ticks(4)
    .runs(2)
    .run();

    println!("Controlled HashMap-order bug (different insertion orders):\n{report}");
    let DeterminismReport::Diverged(divergence) = &report else {
        panic!("the deliberately order-dependent game should diverge");
    };
    assert_eq!(divergence.run, 2);
    assert_eq!(divergence.tick, 1);
    assert!(divergence.diff.entities.iter().any(|entity| {
        entity.components.iter().any(|component| {
            component.name.ends_with("::Enemy")
                && component.fields.iter().any(|field| field.path == "$.moves")
        })
    }));

    let json = serde_json::to_string_pretty(&report)?;
    println!("Machine-readable report:\n{json}");
    let loaded: DeterminismReport = serde_json::from_str(&json)?;
    assert_eq!(report, loaded);
    // Detection is the expected success here; assert_deterministic() would panic.
    Ok(())
}
