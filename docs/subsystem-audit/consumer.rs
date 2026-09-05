use titan::{Component, World};

#[derive(Component)]
struct Position(i32);

// This is also the native entry point's workload. No App or host is constructed.
// The exported function lets Node execute the actual wasm32-unknown-unknown build.
#[unsafe(no_mangle)]
pub extern "C" fn ecs_probe() -> i32 {
    let mut world = World::new();
    assert_eq!(world.entity_count(), 0);
    assert!(world.component_metadata().is_empty());
    assert!(world.resource::<titan::FixedTime>().is_none());
    assert!(world.resource::<titan::ui::UiFocus>().is_none());

    let entity = world.spawn_with((Position(40),));
    world.get_mut::<Position>(entity).unwrap().0 += 2;
    let result = world.get::<Position>(entity).unwrap().0;
    assert_eq!(result, 42);
    assert_eq!(world.component_metadata().len(), 1);
    world.commands().despawn(entity);
    assert!(world.is_alive(entity));
    assert!(world.apply_deferred().is_empty());
    assert_eq!(world.entity_count(), 0);
    result
}
