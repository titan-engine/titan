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
pub(super) struct Captured(pub HashMap<InternedScheduleLabel, Conditions>);

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
        world
            .get_resource_or_init::<Captured>()
            .0
            .insert(self.label, conditions);
        Ok(())
    }
}
