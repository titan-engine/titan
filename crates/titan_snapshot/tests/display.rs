//! Readable matched-diff failure messages without unchanged-world noise.

extern crate alloc;

use alloc::collections::BTreeMap;

use serde_json::json;
use titan_snapshot::{
    DiffConfig, EntityId, EntityMatching, EntitySnapshot, MatchedWorldDiff, SnapshotValue,
    WorldSnapshot,
};

fn id(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

fn named(name: &str) -> EntitySnapshot {
    EntitySnapshot {
        name: Some(name.into()),
        ..Default::default()
    }
}

fn reflected(value: serde_json::Value) -> SnapshotValue {
    SnapshotValue::Reflected { value }
}

fn by_name(before: &WorldSnapshot, after: &WorldSnapshot) -> MatchedWorldDiff {
    before.diff_matched(
        after,
        &DiffConfig::default().with_entity_matching(EntityMatching::ByName),
    )
}

#[test]
fn large_world_prints_only_the_changed_entity_with_its_key_and_both_ids() {
    let mut before = WorldSnapshot::default();
    let mut after = WorldSnapshot::default();
    for index in 0..500 {
        let entity = named(&format!("Unchanged{index}"));
        before.entities.insert(id(10 + index), entity.clone());
        after.entities.insert(id(1000 + index), entity);
    }
    before.entities.insert(
        id(3),
        EntitySnapshot {
            components: BTreeMap::from([("State".into(), reflected(json!({"value": 1})))]),
            ..named("Player")
        },
    );
    after.entities.insert(
        id(5),
        EntitySnapshot {
            components: BTreeMap::from([("State".into(), reflected(json!({"value": 2})))]),
            ..named("Player")
        },
    );
    let diff = by_name(&before, &after);
    assert_eq!(diff.matches.len(), 501);
    let serialized = serde_json::to_value(&diff).unwrap();
    assert_eq!(
        diff.to_string(),
        "~ Name(\"Player\") 3v1 -> 5v1\n    ~ State\n        value: 1 -> 2\n"
    );
    assert!(!diff.to_string().contains("match "));
    assert!(!diff.to_string().contains("Unchanged"));
    assert_eq!(serialized["matches"].as_array().unwrap().len(), 501);
    assert_eq!(serde_json::to_value(&diff).unwrap(), serialized);
    // Even ID-only changes in a large unchanged world produce no match lines.
    after.entities.get_mut(&id(5)).unwrap().components = before.entities[&id(3)].components.clone();
    let unchanged = by_name(&before, &after);
    assert_eq!(unchanged.matches.len(), 501);
    assert_eq!(unchanged.to_string(), "No observable differences.\n");
}

#[test]
fn one_sided_keys_use_display_ids_and_do_not_confuse_overlapping_ids() {
    let before = WorldSnapshot {
        entities: BTreeMap::from([(id(3), named("Old"))]),
        ..Default::default()
    };
    let after = WorldSnapshot {
        entities: BTreeMap::from([(id(3), named("New"))]),
        ..Default::default()
    };
    assert_eq!(
        by_name(&before, &after).to_string(),
        "+ Name(\"New\") (none) -> 3v1\n- Name(\"Old\") 3v1 -> (none)\n"
    );
}

#[test]
fn duplicate_and_unkeyed_diagnostics_are_retained_with_side_correct_inline_keys() {
    let before = WorldSnapshot {
        entities: BTreeMap::from([
            (id(3), named("Duplicate")),
            (id(4), named("Duplicate")),
            (id(5), EntitySnapshot::default()),
        ]),
        ..Default::default()
    };
    let after = WorldSnapshot {
        entities: BTreeMap::from([
            (id(3), named("Duplicate")),
            (id(4), named("New")),
            (id(5), EntitySnapshot::default()),
        ]),
        ..Default::default()
    };
    let diff = by_name(&before, &after);
    let text = diff.to_string();
    assert_eq!(diff.diagnostics.len(), 5);
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("unmatched "))
            .count(),
        5
    );
    for line in [
        "unmatched before 5v1: missing key",
        "unmatched after 5v1: missing key",
        "unmatched before 3v1: duplicate key Name(\"Duplicate\")",
        "unmatched before 4v1: duplicate key Name(\"Duplicate\")",
        "unmatched after 3v1: duplicate key Name(\"Duplicate\")",
        "+ Name(\"New\") (none) -> 4v1",
        "- Name(\"Duplicate\") 4v1 -> (none)",
        "- unkeyed 5v1 -> (none)",
        "+ unkeyed (none) -> 5v1",
    ] {
        assert!(
            text.lines().any(|actual| actual == line),
            "missing {line:?}:\n{text}"
        );
    }
    assert!(!text.contains("EntityId {"));
    assert!(!text.contains("Some("));
}

#[test]
fn component_and_id_keys_preserve_names_component_details_and_resource_output() {
    let before = WorldSnapshot {
        entities: BTreeMap::from([(
            id(3),
            EntitySnapshot {
                components: BTreeMap::from([("StableId".into(), reflected(json!({"number": 7})))]),
                ..named("Player")
            },
        )]),
        resources: BTreeMap::from([("Score".into(), reflected(json!(1)))]),
    };
    let mut after = before.clone();
    after.entities.get_mut(&id(3)).unwrap().name = Some("Hero".into());
    after.resources.insert("Score".into(), reflected(json!(2)));
    let by_component = before.diff_matched(
        &after,
        &DiffConfig::default().with_entity_matching(EntityMatching::ByComponent("StableId".into())),
    );
    assert_eq!(by_component.to_string(), "~ Component(\"StableId\", {\"number\":7}) 3v1 -> 3v1 \"Hero\"\n    name: Some(\"Player\") -> Some(\"Hero\")\n~ resource Score\n    $: 1 -> 2\n");
    let by_id = before.diff_matched(
        &after,
        &DiffConfig::default().with_entity_matching(EntityMatching::ById),
    );
    assert!(by_id
        .to_string()
        .starts_with("~ Id(3v1) 3v1 -> 3v1 \"Hero\"\n"));
    // A resource-only change must not print any of the unchanged entity matches.
    after.entities = before.entities.clone();
    let resource_only = by_name(&before, &after);
    assert_eq!(resource_only.matches.len(), 1);
    assert_eq!(
        resource_only.to_string(),
        "~ resource Score\n    $: 1 -> 2\n"
    );
}

#[test]
fn opaque_key_diagnostics_do_not_claim_an_empty_diff() {
    let snapshot = WorldSnapshot {
        entities: BTreeMap::from([(
            id(3),
            EntitySnapshot {
                components: BTreeMap::from([(
                    "StableId".into(),
                    SnapshotValue::Opaque {
                        reason: "unregistered".into(),
                    },
                )]),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    let mut diff = snapshot.diff_matched(
        &snapshot,
        &DiffConfig::default().with_entity_matching(EntityMatching::ByComponent("StableId".into())),
    );
    // Diagnostics must still be printable even for a manually assembled result
    // containing no structural changes.
    diff.diff.entities.clear();
    assert_eq!(
        diff.to_string(),
        "unmatched before 3v1: opaque key\nunmatched after 3v1: opaque key\n"
    );
}
