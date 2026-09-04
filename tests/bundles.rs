use titan::{App, Bundle, Commands, Component, DeferredOperation, FixedUpdate, World};

#[derive(Component, Debug, PartialEq)]
struct Position(i32);
#[derive(Component, Debug, PartialEq)]
struct Health(u32);
#[derive(Component, Debug, PartialEq)]
struct Marker<const N: usize>;

fn character() -> impl Bundle {
    (Position(5), (Health(100), Marker::<0>))
}

#[test]
fn immediate_bundles_support_reusable_nested_constructors_and_replacement() {
    let mut world = World::new();
    let entity = world.spawn_with(character());
    world
        .insert_bundle(entity, (Position(10), Position(20), ()))
        .unwrap();
    assert_eq!(world.get::<Position>(entity), Some(&Position(20)));
    assert_eq!(world.get::<Health>(entity), Some(&Health(100)));
    assert_eq!(world.get::<Marker<0>>(entity), Some(&Marker));
    assert_eq!(world.component_type_names(entity).len(), 3);
}

#[test]
fn empty_single_and_twelve_component_bundles_are_supported() {
    let mut world = World::new();
    let empty = world.spawn_with(());
    let single = world.spawn_with(Position(1));
    let tuple = world.spawn_with((Health(2),));
    let large = world.spawn_with((
        Marker::<0>,
        Marker::<1>,
        Marker::<2>,
        Marker::<3>,
        Marker::<4>,
        Marker::<5>,
        Marker::<6>,
        Marker::<7>,
        Marker::<8>,
        Marker::<9>,
        Marker::<10>,
        Marker::<11>,
    ));
    assert!(world.is_alive(empty));
    assert!(world.component_type_names(empty).is_empty());
    assert_eq!(world.get::<Position>(single), Some(&Position(1)));
    assert_eq!(world.get::<Health>(tuple), Some(&Health(2)));
    assert_eq!(world.component_type_names(large).len(), 12);
}

#[test]
fn stale_and_reserved_handles_reject_the_entire_bundle() {
    let mut world = World::new();
    let stale = world.spawn_with(character());
    world.despawn(stale);
    let replacement = world.spawn_with(Position(42));
    assert_eq!(stale.index(), replacement.index());
    assert_eq!(
        world
            .insert_bundle(stale, character())
            .unwrap_err()
            .entity(),
        stale
    );
    assert_eq!(world.insert_bundle(stale, ()).unwrap_err().entity(), stale);
    assert_eq!(world.get::<Position>(replacement), Some(&Position(42)));
    assert!(world.get::<Health>(replacement).is_none());
    assert_eq!(world.iter::<Health>().count(), 0);

    let reserved = world.commands().spawn();
    assert_eq!(
        world
            .insert_bundle(reserved, character())
            .unwrap_err()
            .entity(),
        reserved
    );
    assert!(world.apply_deferred().is_empty());
    assert!(world.component_type_names(reserved).is_empty());
}

#[test]
fn deferred_bundle_visibility_and_order_match_individual_commands() {
    let mut world = World::new();
    let entity = {
        let mut commands = world.commands();
        let entity = commands.spawn_with(character());
        commands.insert_bundle(entity, (Health(80), Position(9)));
        commands.insert(entity, Position(10));
        entity
    };
    assert!(!world.is_alive(entity));
    assert_eq!(world.iter::<Position>().count(), 0);
    assert!(world.apply_deferred().is_empty());
    assert_eq!(world.get::<Position>(entity), Some(&Position(10)));
    assert_eq!(world.get::<Health>(entity), Some(&Health(80)));
}

#[test]
fn failed_deferred_bundle_reports_one_error_and_continues_the_batch() {
    let mut world = World::new();
    let doomed = world.spawn_with(character());
    let survivor = {
        let mut commands = world.commands();
        commands.despawn(doomed);
        commands.insert_bundle(doomed, (Position(7), Health(8)));
        commands.spawn_with(character())
    };
    let errors = world.apply_deferred();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].entity(), doomed);
    assert_eq!(errors[0].operation(), DeferredOperation::Insert);
    assert!(!world.is_alive(doomed));
    assert_eq!(world.get::<Health>(survivor), Some(&Health(100)));
    assert_eq!(world.iter::<Position>().count(), 1);
}

#[test]
fn typed_commands_can_spawn_bundles() {
    fn spawn(mut commands: Commands) {
        commands.spawn_with(character());
    }
    let mut app = App::new();
    app.add_systems(FixedUpdate, spawn);
    app.try_advance_fixed(1).unwrap();
    assert_eq!(app.world().iter::<Health>().count(), 1);
}
