//! Acceptance coverage for world capture, filters, serialization, and diffing.

extern crate alloc;

use alloc::collections::BTreeSet;
use bevy_camera::visibility::{InheritedVisibility, ViewVisibility, Visibility};
use bevy_ecs::{prelude::*, reflect::AppTypeRegistry};
use bevy_reflect::{Reflect, TypePath};
use bevy_time::{Fixed, Real, Time, Virtual};
use bevy_transform::components::{GlobalTransform, Transform, TransformTreeChanged};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use titan_snapshot::{
    ChangeKind, DiffConfig, SnapshotConfig, SnapshotValue, TypeFilter, WorldSnapshot,
};

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Position {
    translation: Coordinates,
}

#[derive(Reflect)]
struct Coordinates {
    x: f64,
    y: f64,
}

#[derive(Component)]
struct Opaque;

#[derive(Component, Reflect, Clone)]
#[reflect(Component, opaque)]
struct Unserializable;

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct Score {
    value: u64,
}

#[derive(Resource)]
struct OpaqueResource;

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Collections {
    map: HashMap<String, HashMap<String, u32>>,
    set: HashSet<String>,
    list: Vec<u32>,
}

fn world() -> World {
    let mut world = World::new();
    world.init_resource::<AppTypeRegistry>();
    {
        let mut registry = world.resource::<AppTypeRegistry>().write();
        registry.register::<Position>();
        registry.register::<Name>();
        registry.register::<Score>();
        registry.register::<Collections>();
        registry.register::<Unserializable>();
    }
    world
}

fn capture(world: &World) -> WorldSnapshot {
    WorldSnapshot::capture(world, &SnapshotConfig::default())
}

fn reflected(value: &SnapshotValue) -> &Value {
    let SnapshotValue::Reflected { value } = value else {
        panic!("expected reflected value, got {value:?}");
    };
    value
}

#[test]
fn deterministic_capture_sorted_maps_and_json_round_trip() {
    let mut world = world();
    world.insert_resource(Score { value: 10 });
    let entity = world
        .spawn((
            Name::new("Player"),
            Position {
                translation: Coordinates { x: 0.0, y: 0.0 },
            },
            Collections {
                map: HashMap::from([
                    (
                        "z".into(),
                        HashMap::from([("b".into(), 2), ("a".into(), 1)]),
                    ),
                    ("a".into(), HashMap::new()),
                ]),
                set: HashSet::from(["z".into(), "a".into(), "b".into()]),
                list: vec![3, 2, 1],
            },
            Opaque,
        ))
        .id();
    let first = capture(&world);
    let encoded = serde_json::to_string_pretty(&first).unwrap();
    for _ in 0..8 {
        assert_eq!(
            encoded,
            serde_json::to_string_pretty(&capture(&world)).unwrap()
        );
    }
    let decoded: WorldSnapshot = serde_json::from_str(&encoded).unwrap();
    assert_eq!(first, decoded);
    assert!(first.diff(&decoded, &DiffConfig::default()).is_empty());
    assert_eq!(
        first.entities.len(),
        1,
        "resources must not be duplicated as entities"
    );
    assert_eq!(
        first.entities[&entity.into()].name.as_deref(),
        Some("Player")
    );
    let collections =
        reflected(&first.entities[&entity.into()].components[Collections::type_path()]);
    assert_eq!(collections["set"], json!(["a", "b", "z"]));
    assert_eq!(collections["list"], json!([3, 2, 1]));
    assert_eq!(
        collections["map"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["a", "z"]
    );
    assert_eq!(
        collections["map"]["z"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    // Rebuild hash tables with different insertion/iteration order.
    world.entity_mut(entity).insert(Collections {
        map: HashMap::from([
            ("a".into(), HashMap::new()),
            (
                "z".into(),
                HashMap::from([("a".into(), 1), ("b".into(), 2)]),
            ),
        ]),
        set: HashSet::from(["b".into(), "a".into(), "z".into()]),
        list: vec![3, 2, 1],
    });
    assert_eq!(
        encoded,
        serde_json::to_string_pretty(&capture(&world)).unwrap()
    );
}

#[test]
fn entities_components_resources_and_nested_fields_diff() {
    let mut world = world();
    world.insert_resource(Score { value: 10 });
    let player = world
        .spawn((
            Name::new("Player"),
            Position {
                translation: Coordinates { x: 0.0, y: 0.0 },
            },
        ))
        .id();
    let bullet = world.spawn(Name::new("Bullet")).id();
    let before = capture(&world);
    world.despawn(bullet);
    let new_bullet = world.spawn(Name::new("Bullet")).id();
    world.entity_mut(player).insert(Opaque);
    world.get_mut::<Position>(player).unwrap().translation.y = 2.31;
    world.resource_mut::<Score>().value = 11;
    let after = capture(&world);
    let diff = before.diff(&after, &DiffConfig::default());
    assert!(!diff.is_empty());
    assert!(diff
        .entities
        .iter()
        .any(|d| d.entity == bullet.into() && d.kind == ChangeKind::Removed));
    assert!(diff
        .entities
        .iter()
        .any(|d| d.entity == new_bullet.into() && d.kind == ChangeKind::Added));
    let player_diff = diff
        .entities
        .iter()
        .find(|d| d.entity == player.into())
        .unwrap();
    let position = player_diff
        .components
        .iter()
        .find(|d| d.name == Position::type_path())
        .unwrap();
    assert_eq!(position.kind, ChangeKind::Changed);
    assert_eq!(position.fields.len(), 1);
    assert_eq!(position.fields[0].path, "$.translation.y");
    assert_eq!(position.fields[0].before, Some(json!(0.0)));
    assert_eq!(position.fields[0].after, Some(json!(2.31)));
    assert!(player_diff
        .components
        .iter()
        .any(|d| d.name.ends_with("::Opaque") && d.kind == ChangeKind::Added));
    assert_eq!(diff.resources.len(), 1);
    assert_eq!(diff.resources[0].name, Score::type_path());
    assert_eq!(diff.resources[0].fields[0].path, "$.value");
    let text = diff.to_string();
    assert!(text.contains("translation.y: 0.0 -> 2.31"), "{text}");
    assert!(text.contains("resource snapshot::Score"), "{text}");
    let json = serde_json::to_string(&diff).unwrap();
    assert_eq!(diff, serde_json::from_str(&json).unwrap());
    world.entity_mut(player).remove::<Opaque>();
    let removed = after.diff(&capture(&world), &DiffConfig::default());
    assert_eq!(removed.entities[0].components[0].kind, ChangeKind::Removed);
}

#[test]
fn resource_add_remove_and_opaque_presence() {
    let mut world = world();
    let before = capture(&world);
    world.insert_resource(Score { value: 1 });
    world.insert_resource(OpaqueResource);
    let after = capture(&world);
    let added = before.diff(&after, &DiffConfig::default());
    assert!(added.entities.is_empty());
    assert_eq!(added.resources.len(), 2);
    assert!(added.resources.iter().all(|d| d.kind == ChangeKind::Added));
    assert!(matches!(
        after.resources[core::any::type_name::<OpaqueResource>()],
        SnapshotValue::Opaque { .. }
    ));
    world.remove_resource::<Score>();
    world.remove_resource::<OpaqueResource>();
    let removed = after.diff(&capture(&world), &DiffConfig::default());
    assert!(removed
        .resources
        .iter()
        .all(|d| d.kind == ChangeKind::Removed));
}

#[test]
fn float_tolerance_and_exact_large_integers() {
    let mut world = world();
    let player = world
        .spawn(Position {
            translation: Coordinates { x: 0.0, y: 2.0 },
        })
        .id();
    world.insert_resource(Score {
        value: u64::MAX - 1,
    });
    let before = capture(&world);
    world.get_mut::<Position>(player).unwrap().translation.y += 0.000_001;
    let after = capture(&world);
    assert!(!before.diff(&after, &DiffConfig::default()).is_empty());
    let config = DiffConfig {
        float_tolerance: 0.001,
    };
    assert!(before.diff(&after, &config).is_empty());
    world.resource_mut::<Score>().value += 1;
    let diff = before.diff(&capture(&world), &config);
    assert!(diff.entities.is_empty());
    assert_eq!(diff.resources.len(), 1);
    for tolerance in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(!before
            .diff(
                &after,
                &DiffConfig {
                    float_tolerance: tolerance
                }
            )
            .is_empty());
    }
}

#[test]
fn opaque_unregistered_unserializable_and_no_registry() {
    let mut world = world();
    let entity = world.spawn((Opaque, Unserializable)).id();
    let snapshot = capture(&world);
    let components = &snapshot.entities[&entity.into()].components;
    assert_eq!(components.len(), 2);
    assert!(components
        .values()
        .all(|v| matches!(v, SnapshotValue::Opaque { .. })));
    world.remove_resource::<AppTypeRegistry>();
    let no_registry = capture(&world);
    assert_eq!(no_registry.entities[&entity.into()].components.len(), 2);
    assert!(no_registry.entities[&entity.into()]
        .components
        .values()
        .all(|v| matches!(v, SnapshotValue::Opaque { .. })));
    assert!(snapshot.diff(&snapshot, &DiffConfig::default()).is_empty());
}

#[test]
fn common_bevy_types_and_hierarchy_serialize() {
    let mut world = world();
    {
        let mut registry = world.resource::<AppTypeRegistry>().write();
        registry.register::<Transform>();
        registry.register::<GlobalTransform>();
        registry.register::<TransformTreeChanged>();
        registry.register::<Visibility>();
        registry.register::<InheritedVisibility>();
        registry.register::<ViewVisibility>();
        registry.register::<ChildOf>();
        registry.register::<Children>();
    }
    let parent = world
        .spawn((
            Name::new("Parent"),
            Transform::default(),
            Visibility::Visible,
        ))
        .id();
    let child = world
        .spawn((
            Name::new("Child"),
            Transform::from_xyz(1.0, 2.0, 3.0),
            Visibility::Hidden,
            ChildOf(parent),
        ))
        .id();
    let snapshot = capture(&world);
    assert_eq!(snapshot.entities.len(), 2);
    for entity in [parent, child] {
        let components = &snapshot.entities[&entity.into()].components;
        for path in [
            Name::type_path(),
            Transform::type_path(),
            Visibility::type_path(),
        ] {
            reflected(&components[path]);
        }
        assert!(!components.contains_key(GlobalTransform::type_path()));
    }
    reflected(&snapshot.entities[&child.into()].components[ChildOf::type_path()]);
    reflected(&snapshot.entities[&parent.into()].components[Children::type_path()]);
    let encoded = serde_json::to_string_pretty(&snapshot).unwrap();
    assert_eq!(snapshot, serde_json::from_str(&encoded).unwrap());
}

#[test]
fn filters_default_noise_and_name_metadata() {
    let mut world = world();
    let entity = world
        .spawn((Name::new("Player"), Transform::default(), Opaque))
        .id();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(Time::<Virtual>::default());
    world.insert_resource(Time::<Fixed>::default());
    world.insert_resource(Score { value: 1 });
    let default = capture(&world);
    assert!(!default
        .resources
        .keys()
        .any(|p| p.as_str() == "bevy_time::time::Time" || p.starts_with("bevy_time::time::Time<")));
    assert!(!default
        .resources
        .contains_key(core::any::type_name::<AppTypeRegistry>()));
    assert!(!default.entities[&entity.into()]
        .components
        .contains_key(GlobalTransform::type_path()));
    let all = WorldSnapshot::capture(&world, &SnapshotConfig::all());
    assert!(all.entities[&entity.into()]
        .components
        .contains_key(GlobalTransform::type_path()));
    assert_eq!(
        all.resources
            .keys()
            .filter(|p| p.as_str() == "bevy_time::time::Time"
                || p.starts_with("bevy_time::time::Time<"))
            .count(),
        4
    );
    let mut config = SnapshotConfig {
        components: TypeFilter::only([
            Name::type_path().into(),
            core::any::type_name::<Opaque>().into(),
        ]),
        resources: TypeFilter::only([Score::type_path().into()]),
        exclude_time_resources: false,
    };
    config.components.deny::<Name>();
    let filtered = WorldSnapshot::capture(&world, &config);
    assert_eq!(
        filtered.entities[&entity.into()].name.as_deref(),
        Some("Player")
    );
    assert_eq!(filtered.entities[&entity.into()].components.len(), 1);
    assert_eq!(filtered.resources.len(), 1);
    config.resources.deny::<Score>();
    config.components.allow = Some(BTreeSet::new());
    let empty = WorldSnapshot::capture(&world, &config);
    assert!(empty.resources.is_empty());
    assert!(empty.entities[&entity.into()].components.is_empty());
}

#[test]
fn disabled_entities_are_captured_and_ids_sort_numerically() {
    let mut world = world();
    for _ in 0..12 {
        world.spawn_empty();
    }
    let disabled = world
        .spawn((Name::new("Disabled"), bevy_ecs::entity_disabling::Disabled))
        .id();
    let snapshot = capture(&world);
    assert_eq!(
        snapshot.entities[&disabled.into()].name.as_deref(),
        Some("Disabled")
    );
    let ids = snapshot
        .entities
        .keys()
        .map(|id| id.index)
        .collect::<Vec<_>>();
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    assert!(
        serde_json::from_str::<WorldSnapshot>(r#"{"entities":{"bad":{}},"resources":{}}"#).is_err()
    );
}
