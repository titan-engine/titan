//! Cross-run identity and reference normalization acceptance tests.

use bevy_ecs::{prelude::*, reflect::AppTypeRegistry};
use bevy_reflect::{Reflect, TypePath};
use serde_json::{json, Value};
use titan_snapshot::{
    ChangeKind, DiffConfig, EntityKey, EntityMatchConfig, EntityMatching, MatchProblem,
    MatchedWorldDiff, SnapshotConfig, SnapshotSide, SnapshotValue, TypeFilter, WorldSnapshot,
};

#[derive(Component, Reflect)]
#[reflect(Component)]
struct StableId(u64);

#[derive(Component, Reflect)]
#[reflect(Component)]
struct StructKey {
    namespace: String,
    number: u64,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Links {
    target: Entity,
    nested: Vec<Option<Entity>>,
    ordinary_number: u64,
    ordinary_string: String,
}

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct Selected(Entity);

#[derive(Component)]
struct OpaqueKey;

#[derive(Component, Reflect)]
#[reflect(Component)]
struct ReferenceCollections {
    set: std::collections::HashSet<Entity>,
    map: std::collections::HashMap<Entity, u32>,
    list: Vec<Entity>,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct FloatKey(f64);

#[derive(Clone, PartialEq, Eq, Hash, Reflect)]
struct ReferenceKey {
    label: u32,
    target: Entity,
    #[reflect(ignore)]
    discriminator: u32,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct CompositeCollections {
    set: std::collections::HashSet<ReferenceKey>,
    map: std::collections::HashMap<ReferenceKey, u32>,
}

fn world() -> World {
    let mut world = World::new();
    world.init_resource::<AppTypeRegistry>();
    {
        let mut registry = world.resource::<AppTypeRegistry>().write();
        registry.register::<Name>();
        registry.register::<StableId>();
        registry.register::<StructKey>();
        registry.register::<Links>();
        registry.register::<Selected>();
        registry.register::<ChildOf>();
        registry.register::<Children>();
        registry.register::<ReferenceCollections>();
        registry.register::<FloatKey>();
        registry.register::<CompositeCollections>();
    }
    world
}

fn capture(world: &World) -> WorldSnapshot {
    WorldSnapshot::capture(world, &SnapshotConfig::default())
}

fn config(matching: EntityMatching) -> EntityMatchConfig {
    DiffConfig::default().with_entity_matching(matching)
}

fn named_run(reverse: bool) -> (World, Entity, Entity) {
    let mut world = world();
    let names = if reverse {
        ["Child", "Parent"]
    } else {
        ["Parent", "Child"]
    };
    let first = world.spawn(Name::new(names[0])).id();
    let second = world.spawn(Name::new(names[1])).id();
    let (parent, child) = if reverse {
        (second, first)
    } else {
        (first, second)
    };
    world.entity_mut(child).insert((
        ChildOf(parent),
        Links {
            target: parent,
            nested: vec![Some(parent), None, Some(child)],
            ordinary_number: 42,
            ordinary_string: "0v0".into(),
        },
    ));
    world.insert_resource(Selected(child));
    (world, parent, child)
}

#[test]
fn names_pair_reordered_runs_and_normalize_hierarchy_nested_and_resource_references() {
    let (before, before_parent, _) = named_run(false);
    let (after, after_parent, _) = named_run(true);
    assert_ne!(before_parent, after_parent);
    let before = capture(&before);
    let after = capture(&after);
    assert!(!before.diff(&after, &DiffConfig::default()).is_empty());
    let diff = before.diff_matched(&after, &config(EntityMatching::ByName));
    assert!(diff.is_empty(), "{diff}");
    assert_eq!(diff.matches.len(), 2);
    assert!(diff.diagnostics.is_empty());
    assert!(diff.matches.iter().any(|matched| matched.key
        == EntityKey::Name {
            name: "Parent".into()
        }
        && matched.before == Some(before_parent.into())
        && matched.after == Some(after_parent.into())));
    let encoded = serde_json::to_string_pretty(&diff).unwrap();
    assert!(encoded.contains("Parent"));
    assert_eq!(
        diff,
        serde_json::from_str::<MatchedWorldDiff>(&encoded).unwrap()
    );
    assert_eq!(diff.to_string(), "No observable differences.\n");
    let saved: WorldSnapshot =
        serde_json::from_str(&serde_json::to_string(&before).unwrap()).unwrap();
    assert!(saved
        .diff_matched(&after, &config(EntityMatching::ByName))
        .is_empty());
}

#[test]
fn real_reference_changes_are_shown_through_keys_not_ids() {
    let (before, _, _) = named_run(false);
    let (mut after, _, child) = named_run(true);
    after.get_mut::<Links>(child).unwrap().target = child;
    let diff = capture(&before).diff_matched(&capture(&after), &config(EntityMatching::ByName));
    assert_eq!(diff.diff.entities.len(), 1);
    let change = &diff.diff.entities[0].components[0];
    assert_eq!(change.name, Links::type_path());
    assert_eq!(change.fields.len(), 1);
    let field = &change.fields[0];
    assert_eq!(
        field.before,
        Some(json!({"$titan_entity_key": {"kind": "name", "name": "Parent"}}))
    );
    assert_eq!(
        field.after,
        Some(json!({"$titan_entity_key": {"kind": "name", "name": "Child"}}))
    );
    assert_eq!(field.path, "$.target");
    assert!(diff.to_string().contains("$titan_entity_key"));
}

#[test]
fn ordinary_numbers_and_id_like_strings_are_not_normalized() {
    let (before, _, _) = named_run(false);
    let (mut after, parent, child) = named_run(true);
    let mut links = after.get_mut::<Links>(child).unwrap();
    links.ordinary_number = parent.to_bits();
    links.ordinary_string = titan_snapshot::EntityId::from(parent).to_string();
    let diff = capture(&before).diff_matched(&capture(&after), &config(EntityMatching::ByName));
    let fields = &diff.diff.entities[0].components[0].fields;
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().any(|field| field.path == "$.ordinary_number"));
    assert!(fields.iter().any(|field| field.path == "$.ordinary_string"));
}

#[test]
fn component_keys_support_scalar_and_struct_values_and_ignore_object_order() {
    for matching in [
        EntityMatching::ByComponent(StableId::type_path().into()),
        EntityMatching::ByComponent(StructKey::type_path().into()),
    ] {
        let mut before = world();
        let mut after = world();
        for (world, ids) in [(&mut before, [1, 2]), (&mut after, [2, 1])] {
            for id in ids {
                world.spawn((
                    StableId(id),
                    StructKey {
                        namespace: "players".into(),
                        number: id,
                    },
                ));
            }
        }
        let before = capture(&before);
        let mut after = capture(&after);
        // Exercise canonical keys even for externally constructed/loaded snapshots.
        for entity in after.entities.values_mut() {
            if let SnapshotValue::Reflected {
                value: Value::Object(object),
            } = entity.components.get_mut(StructKey::type_path()).unwrap()
            {
                let namespace = object.remove("namespace").unwrap();
                object.insert("namespace".into(), namespace);
            }
        }
        let diff = before.diff_matched(&after, &config(matching));
        assert!(diff.is_empty(), "{diff}");
        assert_eq!(diff.matches.len(), 2);
        assert!(diff
            .matches
            .iter()
            .all(|matched| matches!(matched.key, EntityKey::Component { .. })));
    }
}

#[test]
fn names_are_available_when_name_components_are_filtered_or_registry_is_absent() {
    let mut before = World::new();
    let mut after = World::new();
    before.spawn(Name::new("A"));
    before.spawn(Name::new("B"));
    after.spawn(Name::new("B"));
    after.spawn(Name::new("A"));
    let capture_config = SnapshotConfig {
        components: TypeFilter::only([]),
        ..Default::default()
    };
    let before = WorldSnapshot::capture(&before, &capture_config);
    let after = WorldSnapshot::capture(&after, &capture_config);
    assert!(before
        .diff_matched(&after, &config(EntityMatching::ByName))
        .is_empty());
}

#[test]
fn duplicate_and_missing_names_never_fall_back_to_ids_or_drop_entities() {
    let mut before = world();
    let mut after = world();
    before.spawn(Name::new("Duplicate"));
    before.spawn(Name::new("Duplicate"));
    before.spawn_empty();
    // A unique counterpart must also be refused when the other side duplicates.
    after.spawn(Name::new("Duplicate"));
    after.spawn_empty();
    let before = capture(&before);
    let after = capture(&after);
    for (before, after) in [(&before, &after), (&after, &before)] {
        let diff = before.diff_matched(after, &config(EntityMatching::ByName));
        assert!(!diff.is_empty());
        assert!(diff.matches.is_empty());
        assert_eq!(diff.diagnostics.len(), 5);
        assert_eq!(diff.diff.entities.len(), 5);
        assert_eq!(
            diff.diff
                .entities
                .iter()
                .filter(|entity| entity.kind == ChangeKind::Removed)
                .count(),
            before.entities.len()
        );
        assert_eq!(
            diff.diff
                .entities
                .iter()
                .filter(|entity| entity.kind == ChangeKind::Added)
                .count(),
            after.entities.len()
        );
        assert_eq!(
            diff.diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic.problem,
                    MatchProblem::DuplicateKey { .. }
                ))
                .count(),
            3
        );
        assert_eq!(
            diff.diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.problem == MatchProblem::MissingKey)
                .count(),
            2
        );
        assert!(diff
            .to_string()
            .contains("duplicate key Name(\"Duplicate\")"));
        assert!(diff.to_string().contains("missing key"));
        assert_eq!(
            diff,
            serde_json::from_str(&serde_json::to_string(&diff).unwrap()).unwrap()
        );
    }
}

#[test]
fn missing_filtered_opaque_and_duplicate_component_keys_are_explicit() {
    let mut world = world();
    world.spawn(StableId(1));
    world.spawn(StableId(1));
    world.spawn(OpaqueKey);
    let snapshot = capture(&world);
    let diff = snapshot.diff_matched(
        &snapshot,
        &config(EntityMatching::ByComponent(StableId::type_path().into())),
    );
    assert_eq!(diff.diagnostics.len(), 6);
    assert!(diff.matches.is_empty());
    let opaque = snapshot.diff_matched(
        &snapshot,
        &config(EntityMatching::ByComponent(
            core::any::type_name::<OpaqueKey>().into(),
        )),
    );
    assert_eq!(
        opaque
            .diagnostics
            .iter()
            .filter(|d| d.problem == MatchProblem::OpaqueKey)
            .count(),
        2
    );
    let filtered = WorldSnapshot::capture(
        &world,
        &SnapshotConfig {
            components: TypeFilter::only([]),
            ..Default::default()
        },
    );
    let diff = filtered.diff_matched(
        &filtered,
        &config(EntityMatching::ByComponent(StableId::type_path().into())),
    );
    assert_eq!(diff.diagnostics.len(), 6);
    assert!(diff
        .diagnostics
        .iter()
        .all(|d| d.problem == MatchProblem::MissingKey));
}

#[test]
fn keyed_lifecycle_name_changes_and_null_keys_roundtrip() {
    let mut before = world();
    let mut after = world();
    let old = before.spawn((Name::new("Old"), StableId(1))).id();
    after.spawn((Name::new("Added"), StableId(2)));
    let new = after.spawn((Name::new("New"), StableId(1))).id();
    before.spawn((Name::new("Removed"), StableId(3)));
    let before = capture(&before);
    let after = capture(&after);
    let diff = before.diff_matched(
        &after,
        &config(EntityMatching::ByComponent(StableId::type_path().into())),
    );
    assert_eq!(diff.diff.entities.len(), 3);
    let changed = diff
        .diff
        .entities
        .iter()
        .find(|entity| entity.kind == ChangeKind::Changed)
        .unwrap();
    assert_eq!(changed.entity, old.into());
    assert_eq!(changed.before_name.as_deref(), Some("Old"));
    assert_eq!(changed.after_name.as_deref(), Some("New"));
    assert!(diff
        .matches
        .iter()
        .any(|matched| matched.before == Some(old.into()) && matched.after == Some(new.into())));
    let mut snapshot = before.clone();
    snapshot.entities.retain(|id, _| *id == old.into());
    snapshot
        .entities
        .get_mut(&old.into())
        .unwrap()
        .components
        .insert(
            StableId::type_path().into(),
            SnapshotValue::Reflected { value: Value::Null },
        );
    let diff = snapshot.diff_matched(
        &snapshot,
        &config(EntityMatching::ByComponent(StableId::type_path().into())),
    );
    assert!(diff.is_empty());
    assert_eq!(
        diff,
        serde_json::from_str(&serde_json::to_string(&diff).unwrap()).unwrap()
    );
}

#[test]
fn defaults_preserve_id_matching_including_generations_and_reference_values() {
    let (before, _, _) = named_run(false);
    let (after, _, _) = named_run(true);
    let before = capture(&before);
    let after = capture(&after);
    let diff = before.diff_matched(&after, &EntityMatchConfig::default());
    assert_eq!(diff.diff, before.diff(&after, &DiffConfig::default()));
    assert!(diff.diagnostics.is_empty());
    assert!(diff
        .matches
        .iter()
        .all(|matched| matches!(matched.key, EntityKey::Id { .. })));
    let mut after = before.clone();
    let (id, entity) = after.entities.pop_first().unwrap();
    after.entities.insert(
        titan_snapshot::EntityId {
            generation: id.generation + 1,
            ..id
        },
        entity,
    );
    assert_eq!(
        before
            .diff_matched(&after, &EntityMatchConfig::default())
            .diff,
        before.diff(&after, &DiffConfig::default())
    );
}

#[test]
fn entity_keyed_maps_and_sets_resort_after_normalization_but_lists_keep_order() {
    let (mut before, before_parent, before_child) = named_run(false);
    let (mut after, after_parent, after_child) = named_run(true);
    for (world, parent, child) in [
        (&mut before, before_parent, before_child),
        (&mut after, after_parent, after_child),
    ] {
        world.entity_mut(child).insert(ReferenceCollections {
            set: std::collections::HashSet::from([parent, child]),
            map: std::collections::HashMap::from([(parent, 1), (child, 2)]),
            list: vec![parent, child],
        });
    }
    let before = capture(&before);
    let after_snapshot = capture(&after);
    assert!(matches!(
        after_snapshot.entities[&after_child.into()].components[ReferenceCollections::type_path()],
        SnapshotValue::Reflected { .. }
    ));
    assert!(before
        .diff_matched(&after_snapshot, &config(EntityMatching::ByName))
        .is_empty());
    after
        .get_mut::<ReferenceCollections>(after_child)
        .unwrap()
        .list
        .reverse();
    let diff = before.diff_matched(&capture(&after), &config(EntityMatching::ByName));
    assert_eq!(diff.diff.entities.len(), 1);
    assert_eq!(diff.diff.entities[0].components.len(), 1);
    assert!(diff.diff.entities[0].components[0]
        .fields
        .iter()
        .all(|field| field.path.starts_with("$.list[")));
    after
        .get_mut::<ReferenceCollections>(after_child)
        .unwrap()
        .map
        .insert(after_parent, 3);
    let diff = before.diff_matched(&capture(&after), &config(EntityMatching::ByName));
    assert!(diff.diff.entities[0].components[0]
        .fields
        .iter()
        .any(|field| field.path.starts_with("$.map")));
}

#[test]
fn composite_reference_keys_are_canonical_and_equal_serialized_map_keys_keep_all_values() {
    let (mut before, before_parent, before_child) = named_run(false);
    let (mut after, after_parent, after_child) = named_run(true);
    for (world, parent, child) in [
        (&mut before, before_parent, before_child),
        (&mut after, after_parent, after_child),
    ] {
        world.entity_mut(child).insert(CompositeCollections {
            set: std::collections::HashSet::from([
                ReferenceKey {
                    label: 2,
                    target: parent,
                    discriminator: 0,
                },
                ReferenceKey {
                    label: 1,
                    target: child,
                    discriminator: 0,
                },
            ]),
            map: std::collections::HashMap::from([
                (
                    ReferenceKey {
                        label: 1,
                        target: parent,
                        discriminator: 1,
                    },
                    10,
                ),
                (
                    ReferenceKey {
                        label: 1,
                        target: parent,
                        discriminator: 2,
                    },
                    20,
                ),
            ]),
        });
    }
    let expected = capture(&before);
    let encoded = serde_json::to_string(&expected).unwrap();
    // Independent hash seeds/iteration orders must not affect equal-key ties.
    for _ in 0..16 {
        before
            .entity_mut(before_child)
            .insert(CompositeCollections {
                set: std::collections::HashSet::from([
                    ReferenceKey {
                        label: 1,
                        target: before_child,
                        discriminator: 0,
                    },
                    ReferenceKey {
                        label: 2,
                        target: before_parent,
                        discriminator: 0,
                    },
                ]),
                map: std::collections::HashMap::from([
                    (
                        ReferenceKey {
                            label: 1,
                            target: before_parent,
                            discriminator: 2,
                        },
                        20,
                    ),
                    (
                        ReferenceKey {
                            label: 1,
                            target: before_parent,
                            discriminator: 1,
                        },
                        10,
                    ),
                ]),
            });
        assert_eq!(encoded, serde_json::to_string(&capture(&before)).unwrap());
    }
    let mut actual = capture(&after);
    let SnapshotValue::Reflected { value } = actual
        .entities
        .get_mut(&after_child.into())
        .unwrap()
        .components
        .get_mut(CompositeCollections::type_path())
        .unwrap()
    else {
        panic!("observable collection")
    };
    assert_eq!(
        value["map"]["$titan_entity_map"].as_array().unwrap().len(),
        2
    );
    // Force map tie order and object insertion order changes in a saved document.
    value["map"]["$titan_entity_map"]
        .as_array_mut()
        .unwrap()
        .reverse();
    fn reverse_object_order(value: &mut Value) {
        match value {
            Value::Object(object) => {
                for value in object.values_mut() {
                    reverse_object_order(value);
                }
                let keys: Vec<_> = object.keys().cloned().collect();
                let mut reordered = serde_json::Map::new();
                for key in keys.into_iter().rev() {
                    reordered.insert(key.clone(), object.remove(&key).unwrap());
                }
                *object = reordered;
            }
            Value::Array(array) => {
                for value in array {
                    reverse_object_order(value);
                }
            }
            _ => {}
        }
    }
    reverse_object_order(value);
    let loaded: WorldSnapshot =
        serde_json::from_str(&serde_json::to_string(&actual).unwrap()).unwrap();
    let diff = expected.diff_matched(&loaded, &config(EntityMatching::ByName));
    assert!(diff.is_empty(), "{diff}");
}

#[test]
fn float_tolerance_never_merges_keys_or_hides_a_changed_reference() {
    let mut before = world();
    let mut after = world();
    for (world, reverse) in [(&mut before, false), (&mut after, true)] {
        let a = world.spawn(FloatKey(1.0)).id();
        let b = world.spawn(FloatKey(1.1)).id();
        world.spawn((
            FloatKey(2.0),
            Links {
                target: if reverse { b } else { a },
                nested: Vec::new(),
                ordinary_number: 0,
                ordinary_string: String::new(),
            },
        ));
    }
    let config = DiffConfig {
        float_tolerance: 0.2,
    }
    .with_entity_matching(EntityMatching::ByComponent(FloatKey::type_path().into()));
    let diff = capture(&before).diff_matched(&capture(&after), &config);
    assert_eq!(diff.matches.len(), 3);
    assert_eq!(diff.diff.entities.len(), 1);
    assert_eq!(diff.diff.entities[0].components[0].fields.len(), 1);
    assert_eq!(
        diff.diff.entities[0].components[0].fields[0].path,
        "$.target"
    );
    let encoded = serde_json::to_string(&config).unwrap();
    assert_eq!(config, serde_json::from_str(&encoded).unwrap());
}

#[test]
fn unkeyed_references_remain_raw_and_diagnosed() {
    let mut world = world();
    let target = world.spawn_empty().id();
    world.insert_resource(Selected(target));
    let snapshot = capture(&world);
    let diff = snapshot.diff_matched(&snapshot, &config(EntityMatching::ByName));
    assert!(!diff.is_empty());
    assert_eq!(diff.diagnostics.len(), 2);
    assert_eq!(diff.diagnostics[0].side, SnapshotSide::Before);
    assert!(diff.diff.resources.is_empty());
    assert!(serde_json::to_string(&snapshot)
        .unwrap()
        .contains("$titan_entity"));
}
