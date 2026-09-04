use titan::{Component, World};

#[derive(Component, Debug, PartialEq)]
struct Position {
    x: i32,
    y: i32,
}

#[derive(Component)]
struct GenericComponent<T: Send + Sync + 'static>(T);

#[test]
fn derived_components_work_from_a_downstream_crate() {
    let mut world = World::new();
    let entity = world.spawn();
    world.insert(entity, Position { x: 3, y: 7 }).unwrap();
    world.insert(entity, GenericComponent(42_u32)).unwrap();

    assert_eq!(
        world.get::<Position>(entity),
        Some(&Position { x: 3, y: 7 })
    );
    assert_eq!(world.get::<GenericComponent<u32>>(entity).unwrap().0, 42);
}

#[test]
fn derived_components_expose_basic_metadata() {
    let metadata = Position::metadata();

    assert!(metadata.type_name.ends_with("::Position"));
    assert_eq!(metadata.size, std::mem::size_of::<Position>());
    assert_eq!(metadata.align, std::mem::align_of::<Position>());
}
