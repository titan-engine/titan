//! Read-only asset diagnostics. Failure history is captured independently of BRP polling.

use alloc::collections::VecDeque;
use bevy_platform::collections::{HashMap, HashSet};
use core::any::TypeId;

use bevy_asset::{
    AssetLoadError, AssetServer, DependencyLoadState, LoadState, RecursiveDependencyLoadState,
    ReflectHandle, UntypedAssetId, UntypedAssetLoadFailedEvent,
};
use bevy_ecs::{prelude::*, reflect::AppTypeRegistry};
use bevy_remote::BrpResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::{check_limit, default_limit, page_with_total, params, value, Page};

/// Maximum number of failures retained, including repeated failures of one asset.
pub const FAILURE_CAPACITY: usize = 256;

#[derive(Resource, Default)]
pub(crate) struct FailureHistory {
    entries: VecDeque<Failure>,
    dropped: u64,
    sequence: u64,
}

struct Failure {
    sequence: u64,
    id: UntypedAssetId,
    path: Option<String>,
    error: String,
}

pub(crate) fn capture_failures(
    events: Option<MessageReader<UntypedAssetLoadFailedEvent>>,
    mut history: ResMut<FailureHistory>,
) {
    let Some(mut events) = events else { return };
    for event in events.read() {
        if history.entries.len() == FAILURE_CAPACITY {
            history.entries.pop_front();
            history.dropped += 1;
        }
        history.sequence += 1;
        let sequence = history.sequence;
        history.entries.push_back(Failure {
            sequence,
            id: event.id,
            // `add_async` failures carry an empty-path sentinel, not a real path.
            // Identify them by error kind so genuine empty paths remain distinguishable.
            path: (!matches!(&event.error, AssetLoadError::AddAsyncError(_)))
                .then(|| event.path.to_string()),
            error: event.error.to_string(),
        });
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum State {
    NotLoaded,
    Loading,
    Loaded,
    Failed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetParams {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default, rename = "type")]
    asset_type: Option<String>,
    #[serde(default)]
    state: Option<State>,
    #[serde(default)]
    path_prefix: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureParams {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default, rename = "type")]
    asset_type: Option<String>,
    #[serde(default)]
    path_prefix: Option<String>,
}

#[derive(Serialize)]
struct Summary {
    id: String,
    path: Option<String>,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    state: Option<State>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AssetRecord {
    #[serde(flatten)]
    summary: Summary,
    server_managed: bool,
    dependency_state: Option<State>,
    dependency_error: Option<String>,
    recursive_dependency_state: Option<State>,
    recursive_dependency_error: Option<String>,
    dependencies: Option<Page<Summary>>,
    dependency_chain: Page<Link>,
    dependency_chain_complete: bool,
}

#[derive(Serialize)]
struct Link {
    parent_id: String,
    #[serde(flatten)]
    dependency: Summary,
}

// Type names and ID formatting do not require visiting asset storage or the server.
// Keep this separate so bounded failure-history queries never build a live inventory.
struct TypeMetadata {
    types: HashMap<TypeId, ReflectHandle>,
}

impl TypeMetadata {
    fn new(world: &World) -> Self {
        let mut types = HashMap::new();
        if let Some(registry) = world.get_resource::<AppTypeRegistry>() {
            for registration in registry.read().iter() {
                if let Some(handle) = registration.data::<ReflectHandle>() {
                    types.insert(handle.asset_type_id(), handle.clone());
                }
            }
        }
        Self { types }
    }

    fn asset_type(&self, id: UntypedAssetId) -> Option<String> {
        self.types
            .get(&id.type_id())
            .map(|handle| handle.asset_type_path().to_owned())
    }

    fn id(&self, id: UntypedAssetId) -> String {
        // IDs are session-local, not paths. The type disambiguates per-type allocators.
        let name = self
            .asset_type(id)
            .unwrap_or_else(|| format!("{:?}", id.type_id()));
        match id {
            UntypedAssetId::Index { index, .. } => format!("{name}:index:{}", index.to_bits()),
            UntypedAssetId::Uuid { uuid, .. } => format!("{name}:uuid:{uuid}"),
        }
    }
}

struct Inventory<'w> {
    world: &'w World,
    server: Option<&'w AssetServer>,
    metadata: TypeMetadata,
    stored: HashSet<UntypedAssetId>,
    ids: HashSet<UntypedAssetId>,
}

impl<'w> Inventory<'w> {
    fn new(world: &'w World) -> Self {
        let metadata = TypeMetadata::new(world);
        let stored: HashSet<_> = metadata
            .types
            .values()
            .flat_map(|handle| handle.ids(world))
            .collect();
        let server = world.get_resource::<AssetServer>();
        let mut ids = stored.clone();
        if let Some(server) = server {
            ids.extend(server.asset_ids());
        }
        Self {
            world,
            server,
            metadata,
            stored,
            ids,
        }
    }

    fn asset_type(&self, id: UntypedAssetId) -> Option<String> {
        self.metadata.asset_type(id)
    }

    fn id(&self, id: UntypedAssetId) -> String {
        self.metadata.id(id)
    }

    fn summary(&self, id: UntypedAssetId) -> Summary {
        let (state, error) = match self.server.and_then(|server| server.get_load_state(id)) {
            Some(LoadState::NotLoaded) => (Some(State::NotLoaded), None),
            Some(LoadState::Loading) => (Some(State::Loading), None),
            Some(LoadState::Loaded) => (Some(State::Loaded), None),
            Some(LoadState::Failed(error)) => (Some(State::Failed), Some(error.to_string())),
            None if self.stored.contains(&id) => (Some(State::Loaded), None),
            None => (None, None),
        };
        Summary {
            id: self.id(id),
            path: self
                .server
                .and_then(|server| server.get_path(id))
                .map(|path| path.to_string()),
            asset_type: self.asset_type(id),
            state,
            error,
        }
    }

    fn dependencies(&self, id: UntypedAssetId) -> Option<Vec<UntypedAssetId>> {
        let mut ids = self
            .metadata
            .types
            .get(&id.type_id())?
            .dependencies(self.world, id)?;
        ids.sort_by_cached_key(|id| self.sort_key(*id));
        ids.dedup();
        Some(ids)
    }

    fn sort_key(&self, id: UntypedAssetId) -> (Option<String>, Option<String>, String) {
        let summary = self.summary(id);
        (summary.path, summary.asset_type, summary.id)
    }

    fn record(&self, id: UntypedAssetId, limit: usize) -> AssetRecord {
        let states = self.server.and_then(|server| server.get_load_states(id));
        let (
            dependency_state,
            dependency_error,
            recursive_dependency_state,
            recursive_dependency_error,
        ) = match states.as_ref() {
            Some((_, direct, recursive)) => {
                let (direct_state, direct_error) = match direct {
                    DependencyLoadState::NotLoaded => (State::NotLoaded, None),
                    DependencyLoadState::Loading => (State::Loading, None),
                    DependencyLoadState::Loaded => (State::Loaded, None),
                    DependencyLoadState::Failed(error) => (State::Failed, Some(error.to_string())),
                };
                let (recursive_state, recursive_error) = match recursive {
                    RecursiveDependencyLoadState::NotLoaded => (State::NotLoaded, None),
                    RecursiveDependencyLoadState::Loading => (State::Loading, None),
                    RecursiveDependencyLoadState::Loaded => (State::Loaded, None),
                    RecursiveDependencyLoadState::Failed(error) => {
                        (State::Failed, Some(error.to_string()))
                    }
                };
                (
                    Some(direct_state),
                    direct_error,
                    Some(recursive_state),
                    recursive_error,
                )
            }
            None => (None, None, None, None),
        };
        let direct = self.dependencies(id);
        let dependencies = direct.as_ref().map(|ids| {
            page_with_total(
                ids.iter().take(limit).map(|id| self.summary(*id)).collect(),
                ids.len(),
            )
        });
        // Breadth-first edges make root -> child -> failed grandchild actionable.
        // Visit each node once, including cycles, and materialize only the page.
        let mut queue = VecDeque::from([id]);
        let mut visited = HashSet::new();
        let mut links = Vec::new();
        let mut total = 0;
        let mut complete = true;
        while let Some(parent) = queue.pop_front() {
            if !visited.insert(parent) {
                continue;
            }
            let Some(children) = self.dependencies(parent) else {
                complete = false;
                continue;
            };
            for child in children {
                total += 1;
                if links.len() < limit {
                    links.push(Link {
                        parent_id: self.id(parent),
                        dependency: self.summary(child),
                    });
                }
                queue.push_back(child);
            }
        }
        AssetRecord {
            summary: self.summary(id),
            server_managed: states.is_some(),
            dependency_state,
            dependency_error,
            recursive_dependency_state,
            recursive_dependency_error,
            dependencies,
            dependency_chain: page_with_total(links, total),
            dependency_chain_complete: complete,
        }
    }
}

fn matches(
    asset_type: &Option<String>,
    path: &Option<String>,
    type_filter: &Option<String>,
    prefix: &Option<String>,
) -> bool {
    type_filter
        .as_ref()
        .is_none_or(|filter| asset_type.as_ref() == Some(filter))
        && prefix
            .as_ref()
            .is_none_or(|prefix| path.as_ref().is_some_and(|path| path.starts_with(prefix)))
}

pub(crate) fn list(In(input): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: AssetParams = params(input)?;
    check_limit(params.limit)?;
    let inventory = Inventory::new(world);
    let mut ids: Vec<_> = inventory
        .ids
        .iter()
        .copied()
        .filter(|id| {
            let summary = inventory.summary(*id);
            params
                .state
                .is_none_or(|state| summary.state == Some(state))
                && matches(
                    &summary.asset_type,
                    &summary.path,
                    &params.asset_type,
                    &params.path_prefix,
                )
        })
        .collect();
    ids.sort_by_cached_key(|id| inventory.sort_key(*id));
    let total = ids.len();
    value(page_with_total(
        ids.into_iter()
            .take(params.limit)
            .map(|id| inventory.record(id, params.limit))
            .collect(),
        total,
    ))
}

#[derive(Serialize)]
struct FailureRecord {
    sequence: u64,
    #[serde(flatten)]
    summary: Summary,
}

#[derive(Serialize)]
struct FailureResponse {
    #[serde(flatten)]
    failures: Page<FailureRecord>,
    capacity: usize,
    dropped: u64,
    history_truncated: bool,
}

pub(crate) fn failures(In(input): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: FailureParams = params(input)?;
    check_limit(params.limit)?;
    let metadata = TypeMetadata::new(world);
    let history = world.resource::<FailureHistory>();
    let matching = history.entries.iter().rev().filter(|failure| {
        matches(
            &metadata.asset_type(failure.id),
            &failure.path,
            &params.asset_type,
            &params.path_prefix,
        )
    });
    let total = matching.clone().count();
    let items = matching
        .take(params.limit)
        .map(|failure| FailureRecord {
            sequence: failure.sequence,
            summary: Summary {
                id: metadata.id(failure.id),
                path: failure.path.clone(),
                asset_type: metadata.asset_type(failure.id),
                state: Some(State::Failed),
                error: Some(failure.error.clone()),
            },
        })
        .collect();
    value(FailureResponse {
        failures: page_with_total(items, total),
        capacity: FAILURE_CAPACITY,
        dropped: history.dropped,
        history_truncated: history.dropped > 0,
    })
}
