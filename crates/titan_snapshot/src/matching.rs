//! Explicit entity identity for cross-run comparisons.

use alloc::{collections::BTreeMap, string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ChangeKind, DiffConfig, EntityId, EntitySnapshot, SnapshotValue, WorldDiff, WorldSnapshot,
};

/// How to identify logical entities. No fuzzy matching or ID fallback is used.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityMatching {
    /// Match complete index/generation IDs within one run (the default).
    #[default]
    ById,
    /// Match exact, case-sensitive `Name` metadata, even if the component is filtered.
    ByName,
    /// Match the exact serialized value of this full component type path.
    ///
    /// Register and capture the component. Struct keys work; numeric tolerance
    /// does not apply to keys. Keys should contain no world-local entity IDs.
    ByComponent(String),
}

/// A matcher plus the existing structural comparison options.
///
/// Kept separate from [`DiffConfig`] to preserve existing struct literals.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityMatchConfig {
    /// Entity identity strategy.
    pub entity_matching: EntityMatching,
    /// Structural value comparison options.
    pub diff: DiffConfig,
}

impl DiffConfig {
    /// Opt into [`WorldSnapshot::diff_matched`] without changing existing callers.
    pub fn with_entity_matching(self, entity_matching: EntityMatching) -> EntityMatchConfig {
        EntityMatchConfig {
            entity_matching,
            diff: self,
        }
    }
}

/// The identity used in a matched diff and in normalized entity references.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityKey {
    /// A world-local identity.
    Id {
        /// Complete index and generation.
        id: EntityId,
    },
    /// Exact `Name` metadata.
    Name {
        /// Case-sensitive name.
        name: String,
    },
    /// A canonical serialized component value.
    Component {
        /// Full captured component path.
        type_path: String,
        /// Exact value, including explicit JSON null if applicable.
        value: Value,
    },
}

/// An unambiguous identity, including entities present on only one side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityMatch {
    /// Key used to pair the entity.
    pub key: EntityKey,
    /// Earlier world-local identity, if present.
    pub before: Option<EntityId>,
    /// Later world-local identity, if present.
    pub after: Option<EntityId>,
}

/// Which input contains an entity that could not be keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSide {
    /// Earlier snapshot.
    Before,
    /// Later snapshot.
    After,
}

/// Why an entity is explicitly unmatched, rather than guessed by ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchProblem {
    /// The name or selected captured component is absent (possibly filtered).
    MissingKey,
    /// The selected component is opaque, so its value cannot be used as a key.
    OpaqueKey,
    /// This key occurs more than once in at least one input.
    /// All occurrences on both sides are unmatched, including unique counterparts.
    DuplicateKey {
        /// Ambiguous identity.
        key: EntityKey,
    },
}

/// An entity excluded from key pairing. Its data still appears as added/removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchDiagnostic {
    /// Input containing this entity.
    pub side: SnapshotSide,
    /// World-local identity.
    pub entity: EntityId,
    /// Reason pairing was refused.
    pub problem: MatchProblem,
}

/// A cross-run diff with explicit identities and unmatched-entity diagnostics.
///
/// `diff.entities[*].entity` uses the earlier ID for pairs and removals, the
/// later ID for additions. Consult `matches` for both IDs and the key. IDs may
/// overlap across runs, so combine the ID with the change kind when looking up
/// an addition/removal. Lists are deterministic; matches include unchanged pairs.
/// Text output shows keys and ID transitions inline only for changed entities,
/// plus all diagnostics. JSON retains the complete match list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchedWorldDiff {
    /// Structural differences; reflected entity references use keys where possible.
    pub diff: WorldDiff,
    /// Every unambiguous identity, sorted by canonical key JSON.
    pub matches: Vec<EntityMatch>,
    /// Every unkeyed/ambiguous entity. Missing/opaque keys are ordered by input
    /// then ID; duplicate groups follow, ordered by canonical key, input, and ID.
    pub diagnostics: Vec<MatchDiagnostic>,
}

impl MatchedWorldDiff {
    /// No observable changes and no matching problems.
    ///
    /// As with [`WorldDiff::is_empty`], opaque internals cannot be compared.
    pub fn is_empty(&self) -> bool {
        self.diff.is_empty() && self.diagnostics.is_empty()
    }
}

impl WorldSnapshot {
    /// Compare logical entities, including across separate world runs.
    ///
    /// Missing, opaque, and duplicate keys never fall back to IDs: earlier
    /// entities are removed, later entities added, and each receives a diagnostic.
    /// Typed `Entity` references captured by this crate are rewritten to unique
    /// keys, also in resources. Lists retain order. Custom serde blobs and older
    /// snapshots without typed reference markers cannot be normalized.
    ///
    /// Normalized sets sort first by their contained entity references in
    /// traversal order, then by the full canonical element value. Reference-keyed
    /// maps sort by the references in each key, then the full canonical key;
    /// the full entry only breaks ties for identical serialized keys. References
    /// in map values never take precedence over keys.
    /// Set elements or map keys with identical references that differ only in
    /// tolerated floats may still pair by full value; this is not tolerance-aware
    /// collection matching.
    ///
    /// ```
    /// use bevy_ecs::prelude::*;
    /// use titan_snapshot::{DiffConfig, EntityMatching, SnapshotConfig, WorldSnapshot};
    ///
    /// let mut before = World::new();
    /// before.spawn(Name::new("Player"));
    /// before.spawn(Name::new("Enemy"));
    /// let mut after = World::new();
    /// after.spawn(Name::new("Enemy"));
    /// after.spawn(Name::new("Player"));
    /// let before = WorldSnapshot::capture(&before, &SnapshotConfig::default());
    /// let after = WorldSnapshot::capture(&after, &SnapshotConfig::default());
    /// let config = DiffConfig::default().with_entity_matching(EntityMatching::ByName);
    /// assert!(before.diff_matched(&after, &config).is_empty());
    /// assert!(!before.diff(&after, &DiffConfig::default()).is_empty());
    /// ```
    pub fn diff_matched(&self, after: &Self, config: &EntityMatchConfig) -> MatchedWorldDiff {
        let mut result = MatchedWorldDiff::default();
        let mut groups: BTreeMap<String, (EntityKey, Vec<EntityId>, Vec<EntityId>)> =
            BTreeMap::new();
        for (side, snapshot) in [(SnapshotSide::Before, self), (SnapshotSide::After, after)] {
            for (&entity, data) in &snapshot.entities {
                match key(entity, data, &config.entity_matching) {
                    Ok(key) => {
                        let group = groups
                            .entry(serde_json::to_string(&key).expect("JSON key"))
                            .or_insert_with(|| (key, Vec::new(), Vec::new()));
                        match side {
                            SnapshotSide::Before => group.1.push(entity),
                            SnapshotSide::After => group.2.push(entity),
                        }
                    }
                    Err(problem) => result.diagnostics.push(MatchDiagnostic {
                        side,
                        entity,
                        problem,
                    }),
                }
            }
        }
        for (_, (key, before, after)) in groups {
            if before.len() > 1 || after.len() > 1 {
                for (side, entities) in
                    [(SnapshotSide::Before, before), (SnapshotSide::After, after)]
                {
                    for entity in entities {
                        result.diagnostics.push(MatchDiagnostic {
                            side,
                            entity,
                            problem: MatchProblem::DuplicateKey { key: key.clone() },
                        });
                    }
                }
            } else {
                result.matches.push(EntityMatch {
                    key,
                    before: before.first().copied(),
                    after: after.first().copied(),
                });
            }
        }
        let before_keys = reference_keys(&result.matches, SnapshotSide::Before);
        let after_keys = reference_keys(&result.matches, SnapshotSide::After);
        let mut before = self.clone();
        let mut after = after.clone();
        // Rewrite only in key modes; ById remains exactly the original diff.
        if config.entity_matching != EntityMatching::ById {
            normalize(&mut before, &before_keys);
            normalize(&mut after, &after_keys);
        }
        result.diff.resources =
            crate::diff::diff_values(&before.resources, &after.resources, config.diff.tolerance());
        for identity in &result.matches {
            let change = entity_diff(
                &before,
                &after,
                identity.before,
                identity.after,
                &config.diff,
            );
            result.diff.entities.extend(change);
        }
        for diagnostic in &result.diagnostics {
            let (before_id, after_id) = match diagnostic.side {
                SnapshotSide::Before => (Some(diagnostic.entity), None),
                SnapshotSide::After => (None, Some(diagnostic.entity)),
            };
            result.diff.entities.extend(entity_diff(
                &before,
                &after,
                before_id,
                after_id,
                &config.diff,
            ));
        }
        // Preserve existing ID ordering in ById and deterministic ordering in key modes.
        result.diff.entities.sort_by_key(|entity| entity.entity);
        result
    }
}

fn entity_diff(
    before: &WorldSnapshot,
    after: &WorldSnapshot,
    before_id: Option<EntityId>,
    after_id: Option<EntityId>,
    config: &DiffConfig,
) -> Option<crate::EntityDiff> {
    crate::diff::diff_entity(
        before_id.or(after_id).expect("at least one entity"),
        before_id.and_then(|id| before.entities.get(&id)),
        after_id.and_then(|id| after.entities.get(&id)),
        config.tolerance(),
    )
}

fn key(
    id: EntityId,
    entity: &EntitySnapshot,
    matching: &EntityMatching,
) -> Result<EntityKey, MatchProblem> {
    match matching {
        EntityMatching::ById => Ok(EntityKey::Id { id }),
        EntityMatching::ByName => entity
            .name
            .clone()
            .map(|name| EntityKey::Name { name })
            .ok_or(MatchProblem::MissingKey),
        EntityMatching::ByComponent(path) => match entity.components.get(path) {
            Some(SnapshotValue::Reflected { value }) => {
                let mut value = value.clone();
                crate::capture::canonicalize(&mut value);
                Ok(EntityKey::Component {
                    type_path: path.clone(),
                    value,
                })
            }
            Some(SnapshotValue::Opaque { .. }) => Err(MatchProblem::OpaqueKey),
            None => Err(MatchProblem::MissingKey),
        },
    }
}

fn reference_keys(matches: &[EntityMatch], side: SnapshotSide) -> BTreeMap<EntityId, Value> {
    matches
        .iter()
        .filter_map(|matched| {
            let id = match side {
                SnapshotSide::Before => matched.before,
                SnapshotSide::After => matched.after,
            }?;
            Some((id, serde_json::to_value(&matched.key).expect("JSON key")))
        })
        .collect()
}

fn normalize(snapshot: &mut WorldSnapshot, keys: &BTreeMap<EntityId, Value>) {
    for value in snapshot
        .entities
        .values_mut()
        .flat_map(|entity| entity.components.values_mut())
        .chain(snapshot.resources.values_mut())
    {
        if let SnapshotValue::Reflected { value } = value {
            normalize_value(value, keys);
        }
    }
}

fn normalize_value(value: &mut Value, keys: &BTreeMap<EntityId, Value>) {
    if let Value::Object(object) = value {
        if object.len() == 1
            && let Some(Value::String(id)) = object.get("$titan_entity")
            && let Ok(id) = EntityId::try_from(id.clone())
            && let Some(key) = keys.get(&id)
        {
            *value = serde_json::json!({"$titan_entity_key": key});
            crate::capture::canonicalize(value);
            return;
        }
        for value in object.values_mut() {
            normalize_value(value, keys);
        }
        // Loaded/external snapshots need not have canonical object insertion
        // order. Canonicalize before collection sorting, including nested keys.
        object.sort_keys();
        if object.len() == 1 {
            if let Some(Value::Array(elements)) = object.get_mut("$titan_entity_set") {
                elements.sort_by_cached_key(collection_sort_key);
            } else if let Some(Value::Array(entries)) = object.get_mut("$titan_entity_map") {
                entries.sort_by_cached_key(map_entry_sort_key);
            }
        }
    } else if let Value::Array(array) = value {
        for value in array {
            normalize_value(value, keys);
        }
    }
}

// normalize_value has already canonicalized objects and nested collections.
// Reference identity must precede numeric data, so tolerated float changes do
// not reorder elements with distinct references. The full value breaks ties
// deterministically without dropping equal-reference entries.
fn collection_sort_key(value: &Value) -> (Vec<String>, String) {
    let mut references = Vec::new();
    collect_entity_references(value, &mut references);
    (references, value.to_string())
}

fn map_entry_sort_key(entry: &Value) -> ((Vec<String>, String), String) {
    // Values cannot decide pairing between distinct keys. Retain the full entry
    // only to order ties when reflection serializes different Rust keys equally.
    (collection_sort_key(&entry[0]), entry.to_string())
}

fn collect_entity_references(value: &Value, references: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if object.len() == 1
                && let Some(reference) = object
                    .get("$titan_entity_key")
                    .or_else(|| object.get("$titan_entity"))
            {
                // A matched key is atomic: do not descend into its fields.
                references.push(reference.to_string());
                return;
            }
            for value in object.values() {
                collect_entity_references(value, references);
            }
        }
        Value::Array(array) => {
            for value in array {
                collect_entity_references(value, references);
            }
        }
        _ => {}
    }
}

impl core::fmt::Display for MatchedWorldDiff {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            return f.write_str("No observable differences.\n");
        }
        // IDs can overlap across runs. Index pairs, additions, and removals
        // separately so an unmatched removal never borrows an addition's key.
        let mut changed = BTreeMap::new();
        let mut added = BTreeMap::new();
        let mut removed = BTreeMap::new();
        for identity in &self.matches {
            match (identity.before, identity.after) {
                (Some(before), Some(_)) => {
                    changed.insert(before, identity);
                }
                (Some(before), None) => {
                    removed.insert(before, identity);
                }
                (None, Some(after)) => {
                    added.insert(after, identity);
                }
                (None, None) => {}
            }
        }
        let mut before_problems = BTreeMap::new();
        let mut after_problems = BTreeMap::new();
        for diagnostic in &self.diagnostics {
            let (side, problems) = match diagnostic.side {
                SnapshotSide::Before => ("before", &mut before_problems),
                SnapshotSide::After => ("after", &mut after_problems),
            };
            problems.insert(diagnostic.entity, &diagnostic.problem);
            write!(f, "unmatched {side} {}: ", diagnostic.entity)?;
            match &diagnostic.problem {
                MatchProblem::MissingKey => f.write_str("missing key")?,
                MatchProblem::OpaqueKey => f.write_str("opaque key")?,
                MatchProblem::DuplicateKey { key } => {
                    write!(f, "duplicate key {}", DisplayKey(key))?;
                }
            }
            writeln!(f)?;
        }
        for entity in &self.diff.entities {
            let (identities, problems) = match entity.kind {
                ChangeKind::Changed => (&changed, None),
                ChangeKind::Added => (&added, Some(&after_problems)),
                ChangeKind::Removed => (&removed, Some(&before_problems)),
            };
            write!(f, "{} ", crate::diff::marker(entity.kind))?;
            let key = if let Some(identity) = identities.get(&entity.entity) {
                write!(f, "{} ", DisplayKey(&identity.key))?;
                display_id(f, identity.before)?;
                f.write_str(" -> ")?;
                display_id(f, identity.after)?;
                Some(&identity.key)
            } else if let Some(problem) = problems.and_then(|problems| problems.get(&entity.entity))
            {
                let key = match problem {
                    MatchProblem::DuplicateKey { key } => {
                        write!(f, "{} ", DisplayKey(key))?;
                        Some(key)
                    }
                    _ => {
                        f.write_str("unkeyed ")?;
                        None
                    }
                };
                let (before, after) = match entity.kind {
                    ChangeKind::Removed => (Some(entity.entity), None),
                    _ => (None, Some(entity.entity)),
                };
                display_id(f, before)?;
                f.write_str(" -> ")?;
                display_id(f, after)?;
                key
            } else {
                // Retain readable output for manually assembled diffs that do
                // not carry matching metadata, rather than inventing a key.
                write!(f, "{}", entity.entity)?;
                None
            };
            if !matches!(key, Some(EntityKey::Name { .. }))
                && let Some(name) = entity.after_name.as_ref().or(entity.before_name.as_ref())
            {
                write!(f, " {name:?}")?;
            }
            writeln!(f)?;
            crate::diff::display_entity_details(f, entity)?;
        }
        for resource in &self.diff.resources {
            crate::diff::display_value(f, "", "resource ", resource)?;
        }
        Ok(())
    }
}

fn display_id(f: &mut core::fmt::Formatter<'_>, id: Option<EntityId>) -> core::fmt::Result {
    match id {
        Some(id) => write!(f, "{id}"),
        None => f.write_str("(none)"),
    }
}

// Private formatting adapter: the public key type and JSON representation stay
// unchanged, while text uses concise names and EntityId's Display form.
struct DisplayKey<'a>(&'a EntityKey);

impl core::fmt::Display for DisplayKey<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            EntityKey::Id { id } => write!(f, "Id({id})"),
            EntityKey::Name { name } => write!(f, "Name({name:?})"),
            EntityKey::Component { type_path, value } => {
                write!(f, "Component({type_path:?}, {value})")
            }
        }
    }
}
