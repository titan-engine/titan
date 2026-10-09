//! Deterministic, readable captures of an ECS world, with structural diffs.
//!
//! Register types in [`AppTypeRegistry`](bevy_ecs::reflect::AppTypeRegistry) with `#[reflect(Component)]` or
//! `#[reflect(Resource)]` to capture their values. Unregistered or unserializable
//! types are recorded as [`SnapshotValue::Opaque`], never silently discarded.
//! Captures are observations, not restorable worlds. Entity IDs match only within
//! the same run. See the crate's README for filters, limitations, and an example.
//!
//! ```
//! use bevy_ecs::{prelude::*, reflect::AppTypeRegistry};
//! use titan_snapshot::{DiffConfig, SnapshotConfig, WorldSnapshot};
//!
//! let mut world = World::new();
//! world.init_resource::<AppTypeRegistry>();
//! let before = WorldSnapshot::capture(&world, &SnapshotConfig::default());
//! world.spawn(Name::new("Player"));
//! let after = WorldSnapshot::capture(&world, &SnapshotConfig::default());
//! assert!(!before.diff(&after, &DiffConfig::default()).is_empty());
//! let json = serde_json::to_string_pretty(&after)?;
//! let loaded: WorldSnapshot = serde_json::from_str(&json)?;
//! assert_eq!(after, loaded);
//! # Ok::<(), serde_json::Error>(())
//! ```

extern crate alloc;

mod capture;
mod diff;

pub use capture::{SnapshotConfig, TypeFilter};
pub use diff::*;

use alloc::{collections::BTreeMap, string::String};
use bevy_ecs::entity::Entity;
use core::fmt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An entity's index and generation, independent of Bevy's internal bit packing.
///
/// JSON represents this as `"12v1"` so IDs also work as map keys. Ordering is
/// numeric (index, then generation), rather than lexicographic string ordering.
/// Keeping identity separate from captured data lets future matchers use user keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EntityId {
    /// Entity index within this world.
    pub index: u32,
    /// Generation, distinguishing reuse of the same index.
    pub generation: u32,
}

impl From<Entity> for EntityId {
    fn from(entity: Entity) -> Self {
        Self {
            index: entity.index().index(),
            generation: entity.generation().to_bits(),
        }
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

impl From<EntityId> for String {
    fn from(id: EntityId) -> Self {
        alloc::format!("{id}")
    }
}

impl TryFrom<String> for EntityId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (index, generation) = value.split_once('v').ok_or("expected indexvgeneration")?;
        Ok(Self {
            index: index.parse().map_err(|_| "invalid entity index")?,
            generation: generation
                .parse()
                .map_err(|_| "invalid entity generation")?,
        })
    }
}

/// The captured value of a component or resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotValue {
    /// A JSON-serializable reflected value.
    Reflected {
        /// Serialized fields, without a redundant type-path wrapper.
        value: Value,
    },
    /// Presence is known, but the contents cannot be observed.
    ///
    /// Changes inside opaque values cannot be detected by a diff.
    Opaque {
        /// Why this value was not captured.
        reason: String,
    },
}

/// Captured metadata and components of one entity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EntitySnapshot {
    /// The entity's `Name`, if present, even when filtered from components.
    pub name: Option<String>,
    /// Components keyed and sorted by full reflected type path (or ECS type name).
    /// Ambiguous paths gain a `[component_id:N]` suffix instead of losing values.
    pub components: BTreeMap<String, SnapshotValue>,
}

/// A stable document describing entities and resources at one moment.
///
/// Save with [`serde_json::to_string_pretty`] and load with [`serde_json::from_str`].
/// Captured maps are recursively sorted, including with `serde_json`'s
/// `preserve_order` feature enabled elsewhere in the dependency graph.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldSnapshot {
    /// Entities sorted numerically by index and generation. Resource entities are
    /// represented only in `resources`, not duplicated here.
    pub entities: BTreeMap<EntityId, EntitySnapshot>,
    /// Send + Sync resources sorted by full type path (or ECS type name), with
    /// `[component_id:N]` suffixes for ambiguous paths.
    /// Non-send resources are outside the ECS resource iterator and not captured.
    pub resources: BTreeMap<String, SnapshotValue>,
}
