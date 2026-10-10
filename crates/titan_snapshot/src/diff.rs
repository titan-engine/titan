//! Structural differences between snapshots captured during the same world run.
//!
//! Entities are matched by their complete index/generation identity, never by
//! name. Reflected values are compared recursively; opaque values expose only
//! their reason, so identical opaque markers do not imply identical internals.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec::Vec,
};
use core::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{EntityId, EntitySnapshot, SnapshotValue, WorldSnapshot};

/// Options for comparing two snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiffConfig {
    /// Absolute tolerance for numbers when at least one operand is a float.
    ///
    /// Differences less than or equal to this tolerance are ignored. Integer
    /// pairs are always compared exactly, including large `u64` values. The
    /// default is zero; negative, infinite, and NaN tolerances act as zero.
    pub float_tolerance: f64,
}

impl DiffConfig {
    pub(crate) fn tolerance(&self) -> f64 {
        if self.float_tolerance.is_finite() && self.float_tolerance >= 0.0 {
            self.float_tolerance
        } else {
            0.0
        }
    }
}

/// The operation represented by a structural difference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// A value exists only in the later snapshot.
    Added,
    /// A value exists only in the earlier snapshot.
    Removed,
    /// A value exists in both snapshots but differs.
    Changed,
}

/// A difference at a field or array element within a reflected value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Unambiguous path rooted at `$`.
    ///
    /// ASCII identifier keys use `.key`; all other keys use JSON-quoted bracket
    /// notation, such as `$["a.b"]`. Array elements use `[index]`. Added or
    /// removed subtrees are reported once at their root rather than per leaf.
    pub path: String,
    /// Whether the field was added, removed, or changed.
    pub kind: ChangeKind,
    /// The earlier value; omitted in JSON for additions (distinct from `null`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_json_value"
    )]
    pub before: Option<Value>,
    /// The later value; omitted in JSON for removals (distinct from `null`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_json_value"
    )]
    pub after: Option<Value>,
}

/// An added, removed, or changed component or resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueDiff {
    /// The full component or resource type name.
    pub name: String,
    /// Whether the component or resource was added, removed, or changed.
    pub kind: ChangeKind,
    /// The earlier captured value, if present.
    pub before: Option<SnapshotValue>,
    /// The later captured value, if present.
    pub after: Option<SnapshotValue>,
    /// Nested differences when both values are reflected.
    ///
    /// This is empty for additions, removals, opaque reason changes, and
    /// transitions between reflected and opaque values. Those differences are
    /// represented by `kind`, `before`, and `after`, not invented field data.
    pub fields: Vec<FieldDiff>,
}

/// Changes to one entity, matched by its complete identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityDiff {
    /// The entity's index and generation, valid only within the same run.
    pub entity: EntityId,
    /// Whether the entity was added, removed, or changed.
    pub kind: ChangeKind,
    /// The earlier entity name, if any.
    pub before_name: Option<String>,
    /// The later entity name, if any.
    pub after_name: Option<String>,
    /// Component changes, ordered by their full type names.
    ///
    /// For added or removed entities, every captured component is respectively
    /// added or removed. A name-only change has an empty component list.
    pub components: Vec<ValueDiff>,
}

/// A machine-readable structural diff, also printable with [`fmt::Display`].
///
/// Lists contain only changes and are deterministic: entities are ordered by
/// identity, types and object fields lexicographically, arrays by index.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldDiff {
    /// Added, removed, or changed entities.
    pub entities: Vec<EntityDiff>,
    /// Added, removed, or changed resources.
    pub resources: Vec<ValueDiff>,
}

impl WorldDiff {
    /// Returns true if no observable differences were found.
    ///
    /// This does not establish equality of opaque internals, which are never
    /// captured and cannot be compared.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.resources.is_empty()
    }
}

impl WorldSnapshot {
    /// Compares this snapshot with a later snapshot from the same world run.
    ///
    /// Matching uses index and generation, not names or cross-run heuristics.
    /// Only captured data is compared; identical opaque markers cannot reveal
    /// changes to their underlying components or resources.
    pub fn diff(&self, after: &Self, config: &DiffConfig) -> WorldDiff {
        let tolerance = config.tolerance();
        let mut diff = WorldDiff {
            entities: Vec::new(),
            resources: diff_values(&self.resources, &after.resources, tolerance),
        };
        for entity in self
            .entities
            .keys()
            .chain(after.entities.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if let Some(change) = diff_entity(
                entity,
                self.entities.get(&entity),
                after.entities.get(&entity),
                tolerance,
            ) {
                diff.entities.push(change);
            }
        }
        diff
    }
}

pub(crate) fn diff_entity(
    entity: EntityId,
    before: Option<&EntitySnapshot>,
    after: Option<&EntitySnapshot>,
    tolerance: f64,
) -> Option<EntityDiff> {
    let empty = BTreeMap::new();
    let before_name = before.and_then(|entity| entity.name.clone());
    let after_name = after.and_then(|entity| entity.name.clone());
    let components = diff_values(
        before.map_or(&empty, |entity| &entity.components),
        after.map_or(&empty, |entity| &entity.components),
        tolerance,
    );
    let kind = match (before, after) {
        (None, Some(_)) => ChangeKind::Added,
        (Some(_), None) => ChangeKind::Removed,
        _ if components.is_empty() && before_name == after_name => return None,
        _ => ChangeKind::Changed,
    };
    Some(EntityDiff {
        entity,
        kind,
        before_name,
        after_name,
        components,
    })
}

pub(crate) fn diff_values(
    before: &BTreeMap<String, SnapshotValue>,
    after: &BTreeMap<String, SnapshotValue>,
    tolerance: f64,
) -> Vec<ValueDiff> {
    let mut changes = Vec::new();
    for name in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let before = before.get(name);
        let after = after.get(name);
        let mut fields = Vec::new();
        let kind = match (before, after) {
            (None, Some(_)) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Removed,
            (
                Some(SnapshotValue::Reflected { value: before }),
                Some(SnapshotValue::Reflected { value: after }),
            ) => {
                diff_fields(
                    Some(before),
                    Some(after),
                    "$".to_owned(),
                    tolerance,
                    &mut fields,
                );
                if fields.is_empty() {
                    continue;
                }
                ChangeKind::Changed
            }
            (
                Some(SnapshotValue::Opaque { reason: before }),
                Some(SnapshotValue::Opaque { reason: after }),
            ) if before == after => continue,
            _ => ChangeKind::Changed,
        };
        changes.push(ValueDiff {
            name: name.clone(),
            kind,
            before: before.cloned(),
            after: after.cloned(),
            fields,
        });
    }
    changes
}

// A present JSON null is Some(Null), whereas a missing property uses the
// serde default None. Option<Value>'s usual deserializer collapses these.
fn present_json_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

fn diff_fields(
    before: Option<&Value>,
    after: Option<&Value>,
    path: String,
    tolerance: f64,
    changes: &mut Vec<FieldDiff>,
) {
    match (before, after) {
        // Logical identity is exact even when component-key values contain
        // floats. Never let structural tolerance hide a changed reference.
        (Some(Value::Object(before)), Some(Value::Object(after)))
            if (before.len() == 1 && before.contains_key("$titan_entity_key"))
                || (after.len() == 1 && after.contains_key("$titan_entity_key")) =>
        {
            if before == after {
                return;
            }
        }
        (Some(Value::Object(before)), Some(Value::Object(after))) => {
            for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
                diff_fields(
                    before.get(key),
                    after.get(key),
                    key_path(&path, key),
                    tolerance,
                    changes,
                );
            }
            return;
        }
        (Some(Value::Array(before)), Some(Value::Array(after))) => {
            for index in 0..before.len().max(after.len()) {
                diff_fields(
                    before.get(index),
                    after.get(index),
                    format!("{path}[{index}]"),
                    tolerance,
                    changes,
                );
            }
            return;
        }
        (Some(Value::Number(before)), Some(Value::Number(after))) => {
            let equal = if before.is_f64() || after.is_f64() {
                match (before.as_f64(), after.as_f64()) {
                    (Some(before), Some(after)) => (before - after).abs() <= tolerance,
                    _ => before == after,
                }
            } else {
                before == after
            };
            if equal {
                return;
            }
        }
        _ if before == after => return,
        _ => {}
    }
    changes.push(FieldDiff {
        path,
        kind: match (before, after) {
            (None, Some(_)) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Removed,
            _ => ChangeKind::Changed,
        },
        before: before.cloned(),
        after: after.cloned(),
    });
}

fn key_path(parent: &str, key: &str) -> String {
    let mut chars = key.chars();
    let identifier = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if identifier {
        format!("{parent}.{key}")
    } else {
        // Serializing a string to JSON cannot fail.
        format!("{parent}[{}]", Value::String(key.to_owned()))
    }
}

impl fmt::Display for WorldDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("No observable differences.\n");
        }
        for entity in &self.entities {
            write!(f, "{} {}", marker(entity.kind), entity.entity)?;
            if let Some(name) = entity.after_name.as_ref().or(entity.before_name.as_ref()) {
                write!(f, " {name:?}")?;
            }
            writeln!(f)?;
            display_entity_details(f, entity)?;
        }
        for resource in &self.resources {
            display_value(f, "", "resource ", resource)?;
        }
        Ok(())
    }
}

pub(crate) fn display_entity_details(
    f: &mut fmt::Formatter<'_>,
    entity: &EntityDiff,
) -> fmt::Result {
    if entity.kind == ChangeKind::Changed && entity.before_name != entity.after_name {
        writeln!(
            f,
            "    name: {:?} -> {:?}",
            entity.before_name, entity.after_name
        )?;
    }
    for component in &entity.components {
        display_value(f, "    ", "", component)?;
    }
    Ok(())
}

pub(crate) fn marker(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "-",
        ChangeKind::Changed => "~",
    }
}

pub(crate) fn display_value(
    f: &mut fmt::Formatter<'_>,
    indent: &str,
    label: &str,
    diff: &ValueDiff,
) -> fmt::Result {
    writeln!(f, "{indent}{} {label}{}", marker(diff.kind), diff.name)?;
    if diff.fields.is_empty() {
        write!(f, "{indent}  ")?;
        display_snapshot_value(f, diff.before.as_ref())?;
        f.write_str(" -> ")?;
        display_snapshot_value(f, diff.after.as_ref())?;
        writeln!(f)?;
    } else {
        for field in &diff.fields {
            let path = field
                .path
                .strip_prefix("$.")
                .or_else(|| field.path.strip_prefix('$'))
                .filter(|path| !path.is_empty())
                .unwrap_or("$");
            write!(f, "{indent}    {path}: ")?;
            display_json_value(f, field.before.as_ref())?;
            f.write_str(" -> ")?;
            display_json_value(f, field.after.as_ref())?;
            writeln!(f)?;
        }
    }
    Ok(())
}

fn display_snapshot_value(
    f: &mut fmt::Formatter<'_>,
    value: Option<&SnapshotValue>,
) -> fmt::Result {
    match value {
        Some(SnapshotValue::Reflected { value }) => write!(f, "{value}"),
        Some(SnapshotValue::Opaque { reason }) => write!(f, "opaque ({reason:?})"),
        None => f.write_str("<absent>"),
    }
}

fn display_json_value(f: &mut fmt::Formatter<'_>, value: Option<&Value>) -> fmt::Result {
    match value {
        Some(value) => write!(f, "{value}"),
        None => f.write_str("<absent>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(before: Value, after: Value, tolerance: f64) -> Vec<FieldDiff> {
        let mut changes = Vec::new();
        diff_fields(
            Some(&before),
            Some(&after),
            "$".to_owned(),
            tolerance,
            &mut changes,
        );
        changes
    }

    #[test]
    fn paths_distinguish_nested_keys_and_punctuation() {
        let changes = fields(
            json!({"a.b": 1, "a": {"b": 1}, "": [null], "q\"": 1}),
            json!({"a.b": 2, "a": {"b": 2}, "": [null, 3], "q\"": 2}),
            0.0,
        );
        let paths: Vec<_> = changes.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["$[\"\"][1]", "$.a.b", "$[\"a.b\"]", "$[\"q\\\"\"]"]);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[0].before, None);
    }

    #[test]
    fn integer_differences_remain_exact_with_tolerance() {
        assert_eq!(
            fields(json!(u64::MAX - 1), json!(u64::MAX), f64::MAX).len(),
            1
        );
        assert_eq!(fields(json!(-1), json!(1), 100.0).len(), 1);
        assert!(fields(json!(1.0), json!(1.25), 0.25).is_empty());
        assert!(fields(json!(1), json!(1.25), 0.25).is_empty());
        assert_eq!(fields(json!(1.0), json!(1.5), 0.25).len(), 1);
    }

    #[test]
    fn absent_and_null_are_distinct() {
        let changes = fields(json!({"removed": null}), json!({"added": null}), 0.0);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[0].after, Some(Value::Null));
        assert_eq!(changes[1].kind, ChangeKind::Removed);
        assert_eq!(changes[1].before, Some(Value::Null));
    }

    #[test]
    fn field_json_roundtrip_preserves_null_and_absence() {
        let changes = fields(
            json!({"removed": null, "changed": null}),
            json!({"added": null, "changed": 1}),
            0.0,
        );
        let json = serde_json::to_string(&changes).unwrap();
        let loaded: Vec<FieldDiff> = serde_json::from_str(&json).unwrap();
        assert_eq!(changes, loaded);
    }

    #[test]
    fn invalid_tolerances_act_as_zero() {
        let before = WorldSnapshot {
            entities: BTreeMap::new(),
            resources: BTreeMap::from([(
                "T".to_owned(),
                SnapshotValue::Reflected { value: json!(1.0) },
            )]),
        };
        let after = WorldSnapshot {
            entities: BTreeMap::new(),
            resources: BTreeMap::from([(
                "T".to_owned(),
                SnapshotValue::Reflected { value: json!(1.25) },
            )]),
        };
        for float_tolerance in [-1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(!before
                .diff(&after, &DiffConfig { float_tolerance })
                .is_empty());
            assert!(before
                .diff(&before, &DiffConfig { float_tolerance })
                .is_empty());
        }
    }

    #[test]
    fn generation_changes_are_removal_and_addition_even_with_same_name() {
        let old = EntityId {
            index: 1,
            generation: 1,
        };
        let new = EntityId {
            index: 1,
            generation: 2,
        };
        let before = WorldSnapshot {
            entities: BTreeMap::from([(
                old,
                EntitySnapshot {
                    name: Some("Player".to_owned()),
                    components: BTreeMap::new(),
                },
            )]),
            resources: BTreeMap::new(),
        };
        let after = WorldSnapshot {
            entities: BTreeMap::from([(
                new,
                EntitySnapshot {
                    name: Some("Player".to_owned()),
                    components: BTreeMap::new(),
                },
            )]),
            resources: BTreeMap::new(),
        };
        let diff = before.diff(&after, &DiffConfig::default());
        assert_eq!(diff.entities.len(), 2);
        assert_eq!(diff.entities[0].entity, old);
        assert_eq!(diff.entities[0].kind, ChangeKind::Removed);
        assert_eq!(diff.entities[1].entity, new);
        assert_eq!(diff.entities[1].kind, ChangeKind::Added);
        let json = serde_json::to_string(&diff).unwrap();
        assert_eq!(diff, serde_json::from_str::<WorldDiff>(&json).unwrap());
        let text = diff.to_string();
        assert!(text.contains("- 1v1 \"Player\""));
        assert!(text.contains("+ 1v2 \"Player\""));
    }

    #[test]
    fn opaque_transitions_do_not_invent_fields() {
        let before = BTreeMap::from([(
            "T".to_owned(),
            SnapshotValue::Opaque {
                reason: "unregistered".to_owned(),
            },
        )]);
        assert!(diff_values(&before, &before, 0.0).is_empty());
        let after = BTreeMap::from([(
            "T".to_owned(),
            SnapshotValue::Reflected {
                value: json!({"x": 1}),
            },
        )]);
        let changes = diff_values(&before, &after, 0.0);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Changed);
        assert!(changes[0].fields.is_empty());
    }
}
