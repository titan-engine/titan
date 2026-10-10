//! Best-effort diagnostics, not evidence that a particular system caused a divergence.

use alloc::collections::{BTreeMap, BTreeSet};
use std::collections::HashMap;

use bevy_ecs::{
    component::ComponentId, reflect::AppTypeRegistry, schedule::Schedules, world::World,
};
use serde::{Deserialize, Serialize};
use titan_snapshot::WorldDiff;

/// An unordered pair of systems that may affect a diverging type.
///
/// These are schedule access conflicts, not proof of the cause of divergence.
/// Conflicts on unrestricted world access are associated with all diverging types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmbiguityHint {
    /// Debug representation of the schedule label.
    pub schedule: String,
    /// Names of the two systems, ordered lexicographically.
    pub systems: [String; 2],
    /// Full snapshot type keys shared by the conflict and the diff.
    pub types: Vec<String>,
    /// Chosen `[before, after]` for `ShuffleAmbiguous`, otherwise `None`.
    /// This is a debugging lead, not proof that the pair caused the divergence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<[String; 2]>,
}

/// The last recorded mutation site of a diverging component or resource.
///
/// Locations describe change detection, including mutable dereferences, rather
/// than necessarily an actual value change. Removed values have no remaining
/// metadata and cannot supply a location in the observed world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeLocationHint {
    /// Snapshot entity identity, or `None` for a resource.
    pub entity: Option<String>,
    /// Full snapshot component or resource type key.
    pub component: String,
    /// Last recorded source location, formatted as `file:line:column`.
    pub location: String,
}

/// Optional debugging leads collected from the diverging run's world.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hints {
    /// Ambiguous system pairs touching diverging types in initialized schedules.
    pub ambiguities: Vec<AmbiguityHint>,
    /// Last mutation sites; empty unless this crate's `track_location` feature is enabled.
    pub change_locations: Vec<ChangeLocationHint>,
}

#[cfg(test)]
pub(crate) fn collect(world: &World, diff: &WorldDiff) -> Hints {
    collect_with_seed(world, diff, None)
}

pub(crate) fn collect_with_seed(
    world: &World,
    diff: &WorldDiff,
    shuffle_seed: Option<u64>,
) -> Hints {
    let diverging: BTreeSet<&str> = diff
        .entities
        .iter()
        .flat_map(|entity| &entity.components)
        .chain(&diff.resources)
        .map(|value| value.name.as_str())
        .collect();
    if diverging.is_empty() {
        return Hints::default();
    }
    let keys = snapshot_type_keys(world);
    let mut hints = Hints::default();
    if let Some(schedules) = world.get_resource::<Schedules>() {
        for (label, schedule) in schedules.iter() {
            // Never initialize or rebuild schedules just to obtain diagnostics.
            let Ok(systems) = schedule.systems() else {
                continue;
            };
            // Schedule::systems() follows executable topological order. Read
            // it AFTER the tick so an in-tick rebuild cannot leave stale hints.
            let names: HashMap<_, _> = systems
                .enumerate()
                .map(|(index, (id, system))| (id, (index, system.name().to_string())))
                .collect();
            let shuffled = shuffle_seed.is_some()
                && schedule.get_build_settings().shuffle_seed == shuffle_seed;
            for (first, second, conflicts) in schedule.graph().conflicting_systems().iter() {
                let mut types: Vec<String> = if conflicts.is_empty() {
                    // An exclusive system or unrestricted entity access can touch
                    // any type. Bevy provides no narrower conflict information.
                    diverging.iter().map(|name| (*name).to_owned()).collect()
                } else {
                    conflicts
                        .iter()
                        .filter_map(|id| keys.get(id))
                        .filter(|name| diverging.contains(name.as_str()))
                        .cloned()
                        .collect()
                };
                if types.is_empty() {
                    continue;
                }
                let (Some(first), Some(second)) = (names.get(first), names.get(second)) else {
                    continue;
                };
                types.sort();
                types.dedup();
                let order = shuffled.then(|| {
                    if first.0 < second.0 {
                        [first.1.clone(), second.1.clone()]
                    } else {
                        [second.1.clone(), first.1.clone()]
                    }
                });
                let mut systems = [first.1.clone(), second.1.clone()];
                systems.sort();
                hints.ambiguities.push(AmbiguityHint {
                    schedule: format!("{label:?}"),
                    systems,
                    types,
                    order,
                });
            }
        }
    }
    // Schedule storage uses hash iteration; report order must not inherit it.
    hints.ambiguities.sort_by(|a, b| {
        (&a.schedule, &a.systems, &a.types, &a.order).cmp(&(
            &b.schedule,
            &b.systems,
            &b.types,
            &b.order,
        ))
    });
    hints.ambiguities.dedup();
    #[cfg(feature = "track_location")]
    {
        hints.change_locations = change_locations(world, diff, &keys);
    }
    hints
}

// Match titan_snapshot's type-key scheme, including reflection's custom type
// paths and colliding dynamic descriptors. Component IDs are world-local.
fn snapshot_type_keys(world: &World) -> BTreeMap<ComponentId, String> {
    let registry = world.get_resource::<AppTypeRegistry>().map(|r| r.read());
    let mut groups: BTreeMap<String, Vec<ComponentId>> = BTreeMap::new();
    for (id, info) in world.components().iter_registered() {
        let path = info
            .type_id()
            .and_then(|id| registry.as_ref()?.get(id))
            .map_or_else(
                || info.name().to_string(),
                |registration| registration.type_info().type_path().to_owned(),
            );
        groups.entry(path).or_default().push(id);
    }
    let mut reserved: BTreeSet<String> = groups.keys().cloned().collect();
    let mut keys = BTreeMap::new();
    for (path, mut ids) in groups {
        ids.sort();
        if ids.len() == 1 {
            keys.insert(ids[0], path);
        } else {
            for id in ids {
                let mut key = format!("{path} [component_id:{}]", id.index());
                while !reserved.insert(key.clone()) {
                    key.push('#');
                }
                keys.insert(id, key);
            }
        }
    }
    keys
}

#[cfg(feature = "track_location")]
fn change_locations(
    world: &World,
    diff: &WorldDiff,
    keys: &BTreeMap<ComponentId, String>,
) -> Vec<ChangeLocationHint> {
    use bevy_ecs::entity::{Entity, EntityGeneration, EntityIndex};

    let ids: BTreeMap<&str, ComponentId> =
        keys.iter().map(|(id, key)| (key.as_str(), *id)).collect();
    let mut hints = Vec::new();
    for entity in &diff.entities {
        let Some(index) = EntityIndex::from_raw_u32(entity.entity.index) else {
            continue;
        };
        let id = Entity::from_index_and_generation(
            index,
            EntityGeneration::from_bits(entity.entity.generation),
        );
        for component in &entity.components {
            if let Some(component_id) = ids.get(component.name.as_str())
                && let Some(location) = last_changed_location(world, id, *component_id)
            {
                hints.push(ChangeLocationHint {
                    entity: Some(entity.entity.to_string()),
                    component: component.name.clone(),
                    location,
                });
            }
        }
    }
    for resource in &diff.resources {
        if let Some(component_id) = ids.get(resource.name.as_str())
            && let Some(entity) = world.resource_entities().get(*component_id)
            && let Some(location) = last_changed_location(world, entity, *component_id)
        {
            hints.push(ChangeLocationHint {
                entity: None,
                component: resource.name.clone(),
                location,
            });
        }
    }
    hints.sort_by(|a, b| {
        (&a.entity, &a.component, &a.location).cmp(&(&b.entity, &b.component, &b.location))
    });
    hints.dedup();
    hints
}

#[cfg(feature = "track_location")]
#[expect(
    unsafe_code,
    reason = "Bevy exposes dynamic last-change metadata only through UnsafeCell; the world is immutably borrowed"
)]
fn last_changed_location(
    world: &World,
    entity: bevy_ecs::entity::Entity,
    component: ComponentId,
) -> Option<String> {
    use bevy_ecs::component::StorageType;

    let entity = world.get_entity(entity).ok()?;
    if !entity.contains_id(component) {
        return None;
    }
    let location = entity.location();
    // This Bevy version has no dynamic get_ref_by_id. Its public storage APIs
    // expose the same last-change metadata that Ref::changed_by reads.
    let cell = match world.components().get_info(component)?.storage_type() {
        StorageType::Table => world
            .storages()
            .tables
            .get(location.table_id)?
            .get_changed_by(component, location.table_row),
        StorageType::SparseSet => world
            .storages()
            .sparse_sets
            .get(component)?
            .get_changed_by(entity.id()),
    }
    .into_option()??;
    // SAFETY: The cell belongs to a live component in this immutably borrowed
    // world. No mutable world/component access can coexist with this borrow, so
    // its last-change metadata cannot be concurrently written. Only the stored
    // shared reference to a static Location is copied; no mutation occurs.
    let changed_by = unsafe { *cell.get() };
    Some(changed_by.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::{prelude::*, schedule::ScheduleLabel};
    use titan_snapshot::{ChangeKind, EntityDiff, EntityId, ValueDiff};

    #[derive(Component)]
    struct Counter(u32);

    #[derive(Resource)]
    struct Total(u32);

    #[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
    struct TestSchedule;

    fn first(mut counters: Query<&mut Counter>) {
        for mut counter in &mut counters {
            counter.0 += 1;
        }
    }

    fn second(mut counters: Query<&mut Counter>) {
        for mut counter in &mut counters {
            counter.0 += 2;
        }
    }

    fn resource_first(mut total: ResMut<Total>) {
        total.0 += 1;
    }

    fn resource_second(mut total: ResMut<Total>) {
        total.0 += 2;
    }

    fn value<T>(kind: ChangeKind) -> ValueDiff {
        ValueDiff {
            name: core::any::type_name::<T>().to_owned(),
            kind,
            before: None,
            after: None,
            fields: Vec::new(),
        }
    }

    fn component_diff(entity: Entity, kind: ChangeKind) -> WorldDiff {
        WorldDiff {
            entities: vec![EntityDiff {
                entity: EntityId::from(entity),
                kind: ChangeKind::Changed,
                before_name: None,
                after_name: None,
                components: vec![value::<Counter>(kind)],
            }],
            resources: Vec::new(),
        }
    }

    #[test]
    fn filters_conflicts_and_includes_added_and_removed_types() {
        let mut world = World::new();
        let entity = world.spawn(Counter(0)).id();
        world.insert_resource(Total(0));
        let mut schedule = Schedule::new(TestSchedule);
        schedule.add_systems((first, second, resource_first, resource_second));
        schedule.run(&mut world);
        world.init_resource::<Schedules>();
        world.resource_mut::<Schedules>().insert(schedule);

        for kind in [ChangeKind::Changed, ChangeKind::Added, ChangeKind::Removed] {
            let hints = collect(&world, &component_diff(entity, kind));
            assert_eq!(hints.ambiguities.len(), 1);
            let hint = &hints.ambiguities[0];
            assert_eq!(hint.schedule, "TestSchedule");
            assert!(hint.systems.iter().any(|name| name.ends_with("::first")));
            assert!(hint.systems.iter().any(|name| name.ends_with("::second")));
            assert_eq!(hint.types, vec![core::any::type_name::<Counter>()]);
        }
        let diff = WorldDiff {
            resources: vec![value::<Total>(ChangeKind::Changed)],
            ..Default::default()
        };
        let hints = collect(&world, &diff);
        assert_eq!(hints.ambiguities.len(), 1);
        assert_eq!(
            hints.ambiguities[0].types,
            vec![core::any::type_name::<Total>()]
        );
        assert!(collect(&world, &WorldDiff::default())
            .ambiguities
            .is_empty());
    }

    #[test]
    fn collecting_is_read_only_and_json_round_trips() {
        let mut world = World::new();
        let entity = world.spawn(Counter(0)).id();
        let diff = component_diff(entity, ChangeKind::Changed);
        let ticks = world.entity(entity).get_change_ticks::<Counter>().unwrap();
        let world_tick = world.read_change_tick();
        let hints = collect(&world, &diff);
        let after = world.entity(entity).get_change_ticks::<Counter>().unwrap();
        assert_eq!(ticks.added, after.added);
        assert_eq!(ticks.changed, after.changed);
        assert_eq!(world_tick, world.read_change_tick());
        let json = serde_json::to_string(&hints).unwrap();
        assert_eq!(hints, serde_json::from_str::<Hints>(&json).unwrap());
        #[cfg(not(feature = "track_location"))]
        assert!(hints.change_locations.is_empty());
    }

    #[test]
    fn shuffled_hints_filter_types_and_accept_older_json() {
        let mut world = World::new();
        let entity = world.spawn(Counter(0)).id();
        world.insert_resource(Total(0));
        world.init_resource::<Schedules>();
        let mut schedule = Schedule::new(TestSchedule);
        schedule.add_systems((first, second, resource_first, resource_second));
        world.resource_mut::<Schedules>().insert(schedule);
        crate::shuffle::configure(&mut world, 42);
        world.run_schedule(TestSchedule);
        let hints = collect_with_seed(
            &world,
            &component_diff(entity, ChangeKind::Changed),
            Some(42),
        );
        assert_eq!(hints.ambiguities.len(), 1);
        assert!(hints
            .ambiguities
            .iter()
            .all(|hint| hint.types == [core::any::type_name::<Counter>()]));
        let order = hints.ambiguities[0].order.as_ref().unwrap();
        let expected: Vec<_> = world
            .resource::<Schedules>()
            .get(TestSchedule)
            .unwrap()
            .systems()
            .unwrap()
            .map(|(_, system)| system.name().to_string())
            .filter(|name| name.ends_with("::first") || name.ends_with("::second"))
            .collect();
        assert_eq!(order.as_slice(), expected);
        let old: AmbiguityHint =
            serde_json::from_str(r#"{"schedule":"Update","systems":["a","b"],"types":[]}"#)
                .unwrap();
        assert_eq!(old.order, None);
    }

    #[test]
    fn shuffled_hints_follow_in_tick_rebuilds_instead_of_old_orders() {
        #[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
        enum Sets {
            A,
            B,
        }
        let mut world = World::new();
        let entity = world.spawn(Counter(0)).id();
        world.init_resource::<Schedules>();
        let mut schedule = Schedule::new(TestSchedule);
        schedule.add_systems((first.in_set(Sets::A), second.in_set(Sets::B)));
        world.resource_mut::<Schedules>().insert(schedule);
        crate::shuffle::configure(&mut world, 42);
        world.run_schedule(TestSchedule);
        let diff = component_diff(entity, ChangeKind::Changed);
        assert_eq!(
            collect_with_seed(&world, &diff, Some(42)).ambiguities.len(),
            1
        );
        // Game code changes dependencies and runs this schedule again during
        // an update. Its old ambiguous order must not survive in diagnostics.
        world
            .resource_mut::<Schedules>()
            .get_mut(TestSchedule)
            .unwrap()
            .configure_sets(Sets::A.before(Sets::B));
        world.run_schedule(TestSchedule);
        assert!(collect_with_seed(&world, &diff, Some(42))
            .ambiguities
            .is_empty());
    }

    #[cfg(feature = "track_location")]
    #[test]
    fn locations_match_component_and_resource_refs_and_skip_removed_values() {
        use bevy_ecs::change_detection::DetectChanges;

        #[derive(Component)]
        #[component(storage = "SparseSet")]
        struct Sparse;

        let mut world = World::new();
        let entity = world.spawn((Counter(0), Sparse)).id();
        world.insert_resource(Total(0));
        world.entity_mut(entity).get_mut::<Counter>().unwrap().0 += 1;
        world.resource_mut::<Total>().0 += 1;
        let mut diff = component_diff(entity, ChangeKind::Changed);
        diff.entities[0]
            .components
            .push(value::<Sparse>(ChangeKind::Added));
        diff.resources.push(value::<Total>(ChangeKind::Changed));
        let expected = [
            (
                Some(entity.to_string()),
                core::any::type_name::<Counter>(),
                world
                    .entity(entity)
                    .get_ref::<Counter>()
                    .unwrap()
                    .changed_by()
                    .into_option()
                    .unwrap()
                    .to_string(),
            ),
            (
                Some(entity.to_string()),
                core::any::type_name::<Sparse>(),
                world
                    .entity(entity)
                    .get_ref::<Sparse>()
                    .unwrap()
                    .changed_by()
                    .into_option()
                    .unwrap()
                    .to_string(),
            ),
            (
                None,
                core::any::type_name::<Total>(),
                world
                    .get_resource_ref::<Total>()
                    .unwrap()
                    .changed_by()
                    .into_option()
                    .unwrap()
                    .to_string(),
            ),
        ];
        let hints = collect(&world, &diff);
        assert_eq!(hints.change_locations.len(), 3);
        for (entity, component, location) in expected {
            assert!(hints.change_locations.contains(&ChangeLocationHint {
                entity,
                component: component.to_owned(),
                location,
            }));
        }
        world.despawn(entity);
        world.remove_resource::<Total>();
        assert!(collect(&world, &diff).change_locations.is_empty());
    }
}
