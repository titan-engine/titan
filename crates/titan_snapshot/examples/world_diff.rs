//! Capture a headless app before and after three simulation ticks.

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use bevy_transform::components::Transform;
use titan_snapshot::{DiffConfig, SnapshotConfig, WorldSnapshot};

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct Score {
    value: u32,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Grounded;

fn tick(mut players: Query<&mut Transform>, mut score: ResMut<Score>) {
    for mut transform in &mut players {
        transform.translation.y += 0.77;
    }
    score.value += 1;
}

#[expect(
    clippy::print_stdout,
    reason = "This example demonstrates the human-readable diff"
)]
fn main() -> Result<(), serde_json::Error> {
    let mut app = App::new();
    app.register_type::<Transform>()
        .register_type::<Name>()
        .register_type::<Score>()
        .register_type::<Grounded>()
        .init_resource::<Score>()
        .add_systems(Update, tick);
    let player = app
        .world_mut()
        .spawn((Name::new("Player"), Transform::default()))
        .id();
    let bullet = app.world_mut().spawn(Name::new("Bullet")).id();
    let config = SnapshotConfig::default();
    let before = WorldSnapshot::capture(app.world(), &config);
    for _ in 0..3 {
        app.update();
    }
    app.world_mut().entity_mut(player).insert(Grounded);
    app.world_mut().despawn(bullet);
    app.world_mut().spawn(Name::new("Bullet"));
    let after = WorldSnapshot::capture(app.world(), &config);
    let diff = before.diff(
        &after,
        &DiffConfig {
            float_tolerance: 0.000_01,
        },
    );
    println!("{diff}");
    // Snapshots are ordinary serde documents, suitable for golden files.
    let json = serde_json::to_string_pretty(&after)?;
    let loaded: WorldSnapshot = serde_json::from_str(&json)?;
    assert!(after.diff(&loaded, &DiffConfig::default()).is_empty());
    Ok(())
}
