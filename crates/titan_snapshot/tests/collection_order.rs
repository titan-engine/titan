//! Normalized collection order must not depend on tolerated numeric changes.

extern crate alloc;

use alloc::collections::BTreeMap;
use serde_json::{json, Value};
use titan_snapshot::{
    DiffConfig, EntityId, EntityMatching, EntitySnapshot, SnapshotValue, WorldSnapshot,
};

fn id(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

fn reference(entity: EntityId) -> Value {
    json!({"$titan_entity": entity.to_string()})
}

fn snapshot(a: EntityId, b: EntityId, collection: Value) -> WorldSnapshot {
    WorldSnapshot {
        entities: BTreeMap::from([
            (
                a,
                EntitySnapshot {
                    name: Some("A".into()),
                    ..Default::default()
                },
            ),
            (
                b,
                EntitySnapshot {
                    name: Some("B".into()),
                    ..Default::default()
                },
            ),
        ]),
        resources: BTreeMap::from([(
            "Collection".into(),
            SnapshotValue::Reflected { value: collection },
        )]),
    }
}

#[test]
fn normalized_set_pairs_distinct_references_before_tolerated_float_values() {
    // The JSON lexical order of full values flips from B,A to A,B, even though
    // each logical element moves by exactly the allowed tolerance.
    let before = snapshot(
        id(1),
        id(2),
        json!({"$titan_entity_set": [
            [2.0, reference(id(1))], [10.0, reference(id(2))],
        ]}),
    );
    let after = snapshot(
        id(2),
        id(1),
        json!({"$titan_entity_set": [
            [3.0, reference(id(2))], [9.0, reference(id(1))],
        ]}),
    );
    let config = DiffConfig {
        float_tolerance: 1.0,
    }
    .with_entity_matching(EntityMatching::ByName);
    // These shapes are also valid saved captures, not just fresh observations.
    let loaded: WorldSnapshot =
        serde_json::from_str(&serde_json::to_string(&before).unwrap()).unwrap();
    let diff = loaded.diff_matched(&after, &config);
    assert!(diff.is_empty(), "{diff}");
    assert!(!before
        .diff_matched(
            &after,
            &DiffConfig::default().with_entity_matching(EntityMatching::ByName)
        )
        .is_empty());
}

#[test]
fn normalized_map_entries_sort_by_nested_matched_and_raw_references_first() {
    // The map keys contain both a changing float and a matched reference. Map
    // values also contain a raw reference to an entity absent from the capture.
    let before = snapshot(
        id(1),
        id(2),
        json!({"$titan_entity_map": [
            [[2.0, {"target": reference(id(1))}], {"nested": [reference(id(999)), 100]}],
            [[10.0, {"target": reference(id(2))}], {"nested": [reference(id(998)), 200]}],
        ]}),
    );
    let after = snapshot(
        id(2),
        id(1),
        json!({"$titan_entity_map": [
            [[3.0, {"target": reference(id(2))}], {"nested": [reference(id(999)), 100]}],
            [[9.0, {"target": reference(id(1))}], {"nested": [reference(id(998)), 200]}],
        ]}),
    );
    let config = DiffConfig {
        float_tolerance: 1.0,
    }
    .with_entity_matching(EntityMatching::ByName);
    let diff = before.diff_matched(&after, &config);
    assert!(diff.is_empty(), "{diff}");
    assert!(!before
        .diff_matched(
            &after,
            &DiffConfig::default().with_entity_matching(EntityMatching::ByName)
        )
        .is_empty());
}

#[test]
fn reference_valued_map_associations_cannot_reorder_distinct_keys_even_with_float_tolerance() {
    // Both keys refer to A and differ only in a scalar field. References in the
    // values must not decide which key is paired, even for tolerated key floats.
    for (first, second) in [(json!(0.0), json!(0.5)), (json!(0), json!(1))] {
        let before = snapshot(
            id(1),
            id(2),
            json!({"$titan_entity_map": [
                [{"field": first, "target": reference(id(1))}, reference(id(1))],
                [{"field": second, "target": reference(id(1))}, reference(id(2))],
            ]}),
        );
        let after = snapshot(
            id(2),
            id(1),
            json!({"$titan_entity_map": [
                [{"field": first, "target": reference(id(2))}, reference(id(1))],
                [{"field": second, "target": reference(id(2))}, reference(id(2))],
            ]}),
        );
        let config = DiffConfig {
            float_tolerance: 1.0,
        }
        .with_entity_matching(EntityMatching::ByName);
        let diff = before.diff_matched(&after, &config);
        assert!(
            !diff.is_empty(),
            "swapped associations for {first} and {second}"
        );
        assert!(diff.diff.entities.is_empty());
        assert_eq!(diff.diff.resources.len(), 1);
        let fields = &diff.diff.resources[0].fields;
        assert_eq!(fields.len(), 2, "{diff}");
        for (index, (before_name, after_name)) in [("A", "B"), ("B", "A")].into_iter().enumerate() {
            assert_eq!(
                fields[index].path,
                format!("$[\"$titan_entity_map\"][{index}][1]")
            );
            assert_eq!(
                fields[index].before,
                Some(json!({"$titan_entity_key": {"kind": "name", "name": before_name}}))
            );
            assert_eq!(
                fields[index].after,
                Some(json!({"$titan_entity_key": {"kind": "name", "name": after_name}}))
            );
        }
    }
}

#[test]
fn raw_references_also_stabilize_order_and_identical_references_use_full_value_ties() {
    for marker in ["$titan_entity_set", "$titan_entity_map"] {
        // Both keys start with the same matched reference, so the later raw
        // reference in each key must be visited too. Sorting by only the first
        // reference or by the float would incorrectly reverse the entries.
        let before = snapshot(
            id(1),
            id(2),
            json!({marker: [
                [[2.0, reference(id(1)), reference(id(999))], 100],
                [[10.0, reference(id(1)), reference(id(998))], 200],
            ]}),
        );
        let after = snapshot(
            id(2),
            id(1),
            json!({marker: [
                [[3.0, reference(id(2)), reference(id(999))], 100],
                [[9.0, reference(id(2)), reference(id(998))], 200],
            ]}),
        );
        let config = DiffConfig {
            float_tolerance: 1.0,
        }
        .with_entity_matching(EntityMatching::ByName);
        assert!(before.diff_matched(&after, &config).is_empty(), "{marker}");

        // Same reference sequence: full values remain the deterministic tie-break
        // and equal-reference entries must not be dropped or left in input order.
        let before = snapshot(
            id(1),
            id(2),
            json!({marker: [
                [[2.0, reference(id(1))], reference(id(999))],
                [[10.0, reference(id(1))], reference(id(999))],
            ]}),
        );
        let after = snapshot(
            id(2),
            id(1),
            json!({marker: [
                [[10.0, reference(id(2))], reference(id(999))],
                [[2.0, reference(id(2))], reference(id(999))],
            ]}),
        );
        assert!(before.diff_matched(&after, &config).is_empty(), "{marker}");
    }
}
