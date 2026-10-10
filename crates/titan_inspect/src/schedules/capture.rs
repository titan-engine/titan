use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use bevy_ecs::{
    prelude::*,
    schedule::{graph::DiGraph, *},
};
use bevy_platform::{collections::HashMap, hash::FixedHasher};
use indexmap::IndexSet;

/// Captures condition names before Bevy moves conditions into its private executor.
/// This pass never changes the graph, dependencies, or world gameplay data.
#[derive(Debug)]
pub(super) struct Capture {
    pub label: InternedScheduleLabel,
    pub lifetime: Arc<()>,
}

#[derive(Resource, Default)]
pub(super) struct Captured(pub HashMap<InternedScheduleLabel, Vec<Conditions>>);

#[derive(Default)]
pub(super) struct Conditions {
    pub systems: BTreeMap<SystemKey, Vec<String>>,
    pub sets: BTreeMap<SystemSetKey, Vec<String>>,
    pub identities: BTreeMap<SystemKey, Option<usize>>,
    pub lifetime: Weak<()>,
}

// Boxed systems keep their allocation when moved from graph to executable.
// Pairing these private addresses with a live build-pass token prevents stale
// capture from being attributed to a replacement schedule, even if its slotmap
// keys (or, after the old schedule is dropped, allocator addresses) are reused.
pub(super) fn identity(system: &bevy_ecs::system::ScheduleSystem) -> Option<usize> {
    // A boxed zero-sized system has no unique allocation (ApplyDeferred is one).
    (size_of_val(&**system) != 0).then(|| core::ptr::from_ref(&**system).cast::<()>() as usize)
}

pub(super) fn for_schedule<'w>(world: &'w World, schedule: &Schedule) -> Option<&'w Conditions> {
    // Use the same instance validation for inspection and automatic observation.
    // A valid pre-finish capture must keep its pass token alive, not be replaced.
    if !schedule
        .systems()
        .ok()?
        .any(|(_, system)| identity(system).is_some())
    {
        return None;
    }
    world
        .get_resource::<Captured>()?
        .0
        .get(&schedule.label())?
        .iter()
        .find(|capture| {
            capture.lifetime.upgrade().is_some()
                && schedule
                    .systems()
                    .expect("checked initialization")
                    .all(|(key, system)| capture.identities.get(&key) == Some(&identity(system)))
        })
}

impl ScheduleBuildPass for Capture {
    type EdgeOptions = ();

    fn add_dependency(&mut self, _from: NodeId, _to: NodeId, _options: Option<&()>) {}

    fn collapse_set(
        &mut self,
        _set: SystemSetKey,
        _systems: &IndexSet<SystemKey, FixedHasher>,
        _dependencies: &DiGraph<NodeId>,
    ) -> impl Iterator<Item = (NodeId, NodeId)> {
        core::iter::empty()
    }

    fn build(
        &mut self,
        world: &mut World,
        graph: &mut ScheduleGraph,
        _dependencies: FlattenedDependencies<'_>,
    ) -> Result<(), ScheduleBuildError> {
        let conditions = Conditions {
            identities: graph
                .systems
                .iter()
                .map(|(key, system, _)| (key, identity(system)))
                .collect(),
            lifetime: Arc::downgrade(&self.lifetime),
            systems: graph
                .systems
                .iter()
                .map(|(key, _, conditions)| {
                    (
                        key,
                        conditions
                            .iter()
                            .map(|c| c.condition.name().to_string())
                            .collect(),
                    )
                })
                .collect(),
            sets: graph
                .system_sets
                .iter()
                .map(|(key, _, conditions)| {
                    (
                        key,
                        conditions
                            .iter()
                            .map(|c| c.condition.name().to_string())
                            .collect(),
                    )
                })
                .collect(),
        };
        let mut captured = world.get_resource_or_init::<Captured>();
        // Transient schedules can use a new label on every build. Prune the
        // entire cache, not just this label, so their dead metadata cannot grow
        // indefinitely. Dead metadata is released on subsequent builds.
        captured.0.retain(|_, candidates| {
            candidates.retain(|candidate| candidate.lifetime.upgrade().is_some());
            !candidates.is_empty()
        });
        let candidates = captured.0.entry(self.label).or_default();
        // A detached, still-live schedule may rebuild under the same label.
        // Replace only this pass's candidate and keep other live instances.
        candidates.retain(|candidate| !candidate.lifetime.ptr_eq(&conditions.lifetime));
        candidates.push(conditions);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    struct Transient(u32);

    #[test]
    fn dead_capture_is_pruned_across_distinct_labels() {
        fn noop() {}
        fn condition() -> bool {
            true
        }
        let mut world = World::new();
        for i in 0..32 {
            let mut schedule = Schedule::new(Transient(i));
            schedule.add_systems(noop.run_if(condition));
            crate::schedules::observe_schedule(&mut schedule);
            schedule.initialize(&mut world).unwrap();
            let captured = world.resource::<Captured>();
            assert_eq!(captured.0.len(), 1);
            assert_eq!(captured.0[&Transient(i).intern()].len(), 1);
            drop(schedule);
        }
    }
}
