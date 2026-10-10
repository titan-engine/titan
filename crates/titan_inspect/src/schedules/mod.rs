//! Schedule inspection and non-mutating condition capture.

mod capture;

use alloc::collections::{BTreeMap, BTreeSet};

use bevy_ecs::{
    prelude::*,
    schedule::{
        graph::{DiGraph, Direction},
        *,
    },
};
use bevy_remote::BrpResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::protocol::{
    check_limit, default_limit, invalid, names, page, params, value, ListParams,
};
use capture::{identity, Capture, Captured};

/// Installs non-mutating run-condition capture for a schedule's next build.
///
/// [`crate::InspectPlugin`] calls this for existing schedules at plugin finish.
/// Call it yourself for schedules created or replaced later, before initialization.
/// Installing after a build cannot recover conditions until the next rebuild;
/// inspection reports them as unavailable rather than inventing an empty list.
/// Install after all build passes that modify run conditions. Repeated calls
/// move capture to the end of the pass list, without adding duplicate passes.
pub fn observe_schedule(schedule: &mut Schedule) {
    schedule.remove_build_pass::<Capture>();
    schedule.add_build_pass(Capture {
        label: schedule.label(),
        lifetime: alloc::sync::Arc::new(()),
    });
}

pub(crate) fn list(In(input): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: ListParams = params(input)?;
    check_limit(params.limit)?;
    let Some(schedules) = world.get_resource::<Schedules>() else {
        return value(page(Vec::<Value>::new(), params.limit));
    };
    let mut items: Vec<_> = schedules
        .iter()
        .map(|(label, schedule)| {
            json!({
                "name": format!("{label:?}"),
                "status": status(schedule),
                "system_count": schedule.systems_len(),
                "executor_kind": null,
                "executor_kind_available": false,
            })
        })
        .collect();
    items.extend(
        schedules
            .get_temporarily_removed()
            .into_iter()
            .map(|label| {
                json!({"name": format!("{label:?}"), "status": "running",
            "system_count": null, "executor_kind": null, "executor_kind_available": false})
            }),
    );
    items.sort_by_key(Value::to_string);
    // Sort primarily by label, not by JSON object key order.
    items.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    value(page(items, params.limit))
}

fn status(schedule: &Schedule) -> &'static str {
    if schedule.systems().is_err() {
        "uninitialized"
    } else if schedule.is_changed() {
        "pending_rebuild"
    } else {
        "initialized"
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleParams {
    schedule: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn find<'a>(world: &'a World, name: &str) -> BrpResult<&'a Schedule> {
    let schedules = world
        .get_resource::<Schedules>()
        .ok_or_else(|| invalid("No schedules exist"))?;
    let mut matches = schedules
        .iter()
        .filter(|(label, _)| format!("{label:?}") == name);
    let found = matches.next().map(|(_, schedule)| schedule);
    let running = schedules
        .get_temporarily_removed()
        .into_iter()
        .filter(|label| format!("{label:?}") == name)
        .count();
    if matches.next().is_some() || usize::from(found.is_some()) + running > 1 {
        return Err(invalid("Schedule name is not unique"));
    }
    if running == 1 {
        return Err(invalid(
            "Schedule is currently running and temporarily unavailable",
        ));
    }
    found.ok_or_else(|| invalid(format!("Unknown schedule: {name}")))
}

// Walk iteratively to support deep set hierarchies without recursion. A visited
// set also tolerates cycles in graphs that have not successfully built yet.
fn reachable(graph: &DiGraph<NodeId>, start: NodeId, direction: Direction) -> BTreeSet<NodeId> {
    let mut visited = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(node) = pending.pop() {
        for neighbor in graph.neighbors_directed(node, direction) {
            if visited.insert(neighbor) {
                pending.push(neighbor);
            }
        }
    }
    visited
}

// Source nodes ascend membership to find applicable declarations. Declaration
// nodes follow explicit dependency paths (including through empty sets). Member
// nodes descend membership, but cannot continue declarations until an actual
// system witnesses the transition. Thus an empty child set cannot fabricate an
// ordering from its parent. The graph stays O(V + E), rather than materializing
// the potentially quadratic transitive closure.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OrderingPhase {
    Source,
    Declaration,
    Member,
}

type OrderingNode = (NodeId, OrderingPhase);

#[derive(Default)]
struct OrderingGraph {
    before: BTreeMap<OrderingNode, Vec<OrderingNode>>,
    after: BTreeMap<OrderingNode, Vec<OrderingNode>>,
}

impl OrderingGraph {
    fn new(graph: &ScheduleGraph, keys: impl Iterator<Item = SystemKey>) -> Self {
        let mut result = Self::default();
        let mut edge = |a, b| {
            result.before.entry(a).or_default().push(b);
            result.after.entry(b).or_default().push(a);
        };
        use OrderingPhase::{Declaration, Member, Source};
        for (parent, child) in graph.hierarchy().graph().all_edges() {
            edge((child, Source), (parent, Source));
            edge((parent, Declaration), (child, Member));
            edge((parent, Member), (child, Member));
        }
        for (a, b) in graph.dependency().graph().all_edges() {
            edge((a, Source), (b, Declaration));
            edge((a, Declaration), (b, Declaration));
        }
        for key in keys {
            edge(
                (NodeId::System(key), Member),
                (NodeId::System(key), Declaration),
            );
            edge(
                (NodeId::System(key), Declaration),
                (NodeId::System(key), Source),
            );
        }
        result
    }

    fn related(&self, key: SystemKey, before: bool) -> BTreeSet<SystemKey> {
        let edges = if before { &self.before } else { &self.after };
        let mut visited = BTreeSet::new();
        let start = if before {
            OrderingPhase::Source
        } else {
            OrderingPhase::Declaration
        };
        let target = if before {
            OrderingPhase::Declaration
        } else {
            OrderingPhase::Source
        };
        let mut pending = vec![(NodeId::System(key), start)];
        let mut systems = BTreeSet::new();
        while let Some(node) = pending.pop() {
            for &neighbor in edges.get(&node).into_iter().flatten() {
                if visited.insert(neighbor) {
                    if neighbor.1 == target
                        && let NodeId::System(other) = neighbor.0
                        && other != key
                    {
                        systems.insert(other);
                    }
                    pending.push(neighbor);
                }
            }
        }
        systems
    }
}

pub(crate) fn systems(In(input): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: ScheduleParams = params(input)?;
    check_limit(params.limit)?;
    let schedule = find(world, &params.schedule)?;
    let state = status(schedule);
    if state != "initialized" {
        return Ok(json!({"schedule": params.schedule, "status": state, "systems": null}));
    }
    let graph = schedule.graph();
    let mut systems: Vec<_> = schedule
        .systems_with_access()
        .expect("checked initialization")
        .collect();
    let captured = world
        .get_resource::<Captured>()
        .and_then(|c| c.0.get(&schedule.label()))
        .and_then(|candidates| {
            candidates.iter().find(|capture| {
                capture.lifetime.upgrade().is_some()
                // At least one actual allocation must witness schedule identity.
                && systems.iter().any(|(_, system)| identity(system.system()).is_some())
                && systems.iter().all(|(key, system)| {
                    capture.identities.get(key) == Some(&identity(system.system()))
                })
            })
        });
    let system_names: BTreeMap<_, _> = systems
        .iter()
        .map(|(key, system)| (*key, system.system().name().to_string()))
        .collect();
    let declared = OrderingGraph::new(graph, systems.iter().map(|(key, _)| *key));
    let total = systems.len();
    // Keys are private tie-breakers for repeated names, assigned by declaration
    // instance order. Compute expensive nested details only for the outer page.
    systems.sort_by(|(a, _), (b, _)| (&system_names[a], a).cmp(&(&system_names[b], b)));
    systems.truncate(params.limit);
    let mut items = Vec::new();
    for (key, system) in &systems {
        let node = NodeId::System(*key);
        let ancestors = reachable(graph.hierarchy().graph(), node, Direction::Incoming);
        let set_names = ancestors
            .iter()
            .filter_map(NodeId::as_set)
            .filter_map(|key| graph.system_sets.get(key))
            .map(|set| format!("{set:?}"))
            .collect();
        let conditions = captured.map(|capture| {
            let mut conditions = capture.systems.get(key).cloned().unwrap_or_default();
            for set in ancestors.iter().filter_map(NodeId::as_set) {
                conditions.extend(capture.sets.get(&set).into_iter().flatten().cloned());
            }
            names(conditions, params.limit)
        });
        let ordered = |before| {
            names(
                declared
                    .related(*key, before)
                    .into_iter()
                    .filter_map(|key| system_names.get(&key).cloned())
                    .collect(),
                params.limit,
            )
        };
        items.push(json!({
            "name": system_names[key],
            "exclusive": system.access().is_exclusive(),
            "sets": names(set_names, params.limit),
            "run_conditions": conditions,
            "before": ordered(true),
            "after": ordered(false),
        }));
    }
    Ok(json!({"schedule": params.schedule, "status": state,
        "ordering": "declared_transitive", "systems": crate::protocol::page_with_total(items, total)}))
}

pub(crate) fn ambiguities(In(input): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: ScheduleParams = params(input)?;
    check_limit(params.limit)?;
    let schedule = find(world, &params.schedule)?;
    let state = status(schedule);
    if state != "initialized" {
        return Ok(json!({"schedule": params.schedule, "status": state, "ambiguities": null}));
    }
    let system_names: BTreeMap<_, _> = schedule
        .systems()
        .expect("checked initialization")
        .map(|(key, system)| (key, system.name().to_string()))
        .collect();
    let mut items = Vec::new();
    for (a, b, conflicts) in &schedule.graph().conflicting_systems().0 {
        let mut pair = [system_names[a].clone(), system_names[b].clone()];
        pair.sort();
        let types = conflicts
            .iter()
            .filter_map(|id| world.components().get_info(*id))
            .map(|info| info.name().to_string())
            .collect();
        items.push(json!({"systems": pair, "conflicts": names(types, params.limit), "world_access": conflicts.is_empty()}));
    }
    items.sort_by_key(Value::to_string);
    items.sort_by(|a, b| {
        (a["systems"][0].as_str(), a["systems"][1].as_str())
            .cmp(&(b["systems"][0].as_str(), b["systems"][1].as_str()))
    });
    Ok(
        json!({"schedule": params.schedule, "status": state, "ambiguities": page(items, params.limit)}),
    )
}
