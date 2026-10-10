//! Cross-run identity coverage without relying on thread timing or random order.

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_reflect::{Reflect, TypePath};
use serde_json::json;
use titan_determinism::{
    DeterminismCheck, DeterminismReport, DiffConfig, EntityMatching, SnapshotConfig, Variant,
};
use titan_snapshot::{EntityKey, MatchProblem, TypeFilter};
use titan_test::Sim;

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Health {
    value: f64,
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct StableKey {
    number: u32,
}

#[derive(Resource)]
struct CallingThread(std::thread::ThreadId);

fn capture_config() -> SnapshotConfig {
    SnapshotConfig {
        components: TypeFilter::only([
            Health::type_path().into(),
            StableKey::type_path().into(),
            ChildOf::type_path().into(),
        ]),
        resources: TypeFilter::only([]),
        ..Default::default()
    }
}

// The identical factory deliberately allocates in opposite orders on caller and
// worker threads. This models a legitimate executor-dependent allocation order
// without a flaky race, sleep, or assumption about which system runs first.
fn scenario(diverge_at: u64) -> Sim {
    let caller = std::thread::current().id();
    let mut sim = Sim::new(|app| {
        app.register_type::<Health>()
            .register_type::<StableKey>()
            .register_type::<ChildOf>()
            .insert_resource(CallingThread(caller))
            .add_systems(
                Update,
                move |mut commands: Commands,
                      thread: Res<CallingThread>,
                      mut tick: Local<u64>,
                      mut entities: Query<(&StableKey, &mut Health)>| {
                    *tick += 1;
                    let worker = std::thread::current().id() != thread.0;
                    if *tick == 1 {
                        let first = commands.spawn_empty().id();
                        let second = commands.spawn_empty().id();
                        let (parent, child) = if worker {
                            (second, first)
                        } else {
                            (first, second)
                        };
                        commands.entity(parent).insert((
                            Name::new("Parent"),
                            StableKey { number: 1 },
                            Health { value: 10.0 },
                        ));
                        commands.entity(child).insert((
                            Name::new("Child"),
                            StableKey { number: 2 },
                            Health { value: 20.0 },
                            ChildOf(parent),
                        ));
                    }
                    if worker && *tick == diverge_at {
                        for (key, mut health) in &mut entities {
                            if key.number == 2 {
                                health.value += 1.0;
                            }
                        }
                    }
                },
            );
    });
    key_test_infrastructure(&mut sim);
    sim
}

// Sim includes synthetic input and plugin infrastructure entities, even with
// component filters. Give this fixture's fixed infrastructure explicit keys;
// gameplay entities are only created later, by the thread-dependent system.
fn key_test_infrastructure(sim: &mut Sim) {
    let mut ids: Vec<_> = sim
        .world()
        .iter_entities()
        .filter(|e| !e.contains_id(bevy_ecs::resource::IS_RESOURCE))
        .map(|e| e.id())
        .collect();
    ids.sort();
    for (index, id) in ids.into_iter().enumerate() {
        sim.world_mut().entity_mut(id).insert((
            Name::new(format!("Infrastructure {index}")),
            StableKey {
                number: 100 + index as u32,
            },
        ));
    }
}

fn check(matching: EntityMatching, diverge_at: u64) -> DeterminismReport {
    DeterminismCheck::new(|| scenario(diverge_at))
        .ticks(6)
        .variant(Variant::MultiThreaded)
        .snapshot_config(capture_config())
        .entity_matching(matching)
        // Setting tolerance afterwards must not reset the matcher.
        .diff_config(DiffConfig {
            float_tolerance: 0.25,
        })
        .run()
}

#[test]
fn entity_matching_thread_dependent_spawn_order_passes_by_name_but_fails_by_id() {
    // Default ID matching remains strict, including allocation order.
    let report = DeterminismCheck::new(|| scenario(u64::MAX))
        .ticks(6)
        .variant(Variant::MultiThreaded)
        .snapshot_config(capture_config())
        .run();
    let DeterminismReport::Diverged(divergence) = report else {
        panic!("ID matching must catch the changed allocation order");
    };
    assert_eq!(divergence.tick, 1);
    assert_eq!(divergence.parameters.entity_matching, EntityMatching::ById);
    assert!(matches!(divergence.matches[0].key, EntityKey::Id { .. }));
    assert_eq!(
        check(EntityMatching::ByName, u64::MAX),
        DeterminismReport::Deterministic {
            runs: 2,
            ticks: 6,
            entity_matching: EntityMatching::ByName,
        }
    );
}

#[test]
fn entity_matching_real_divergence_is_at_the_correct_tick_with_keys_in_text_and_json() {
    let report = check(EntityMatching::ByName, 3);
    let DeterminismReport::Diverged(divergence) = &report else {
        panic!("expected a real health divergence");
    };
    assert_eq!((divergence.run, divergence.tick), (2, 3));
    assert_eq!(
        divergence.parameters.entity_matching,
        EntityMatching::ByName
    );
    assert!(divergence.diagnostics.is_empty());
    let entity = &divergence.diff.entities[0];
    assert_eq!(divergence.diff.entities.len(), 1);
    assert_eq!(entity.components.len(), 1);
    assert_eq!(entity.components[0].name, Health::type_path());
    let field = &entity.components[0].fields[0];
    assert_eq!(field.path, "$.value");
    assert_eq!(field.before, Some(json!(20.0)));
    assert_eq!(field.after, Some(json!(21.0)));
    let matched = divergence
        .matches
        .iter()
        .find(|m| {
            m.key
                == EntityKey::Name {
                    name: "Child".into(),
                }
        })
        .unwrap();
    assert_eq!(matched.before, Some(entity.entity));
    assert_ne!(matched.before, matched.after);
    let text = report.to_string();
    assert!(text.contains("entity matcher: ByName"));
    assert!(text.contains("Name(\"Child\")"));
    assert!(text.contains(&format!(
        "{} -> {}",
        matched.before.unwrap(),
        matched.after.unwrap()
    )));
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["Diverged"]["parameters"]["entity_matching"], "by_name");
    assert_eq!(json["Diverged"]["matches"][0]["key"]["kind"], "name");
    assert_eq!(
        serde_json::from_value::<DeterminismReport>(json).unwrap(),
        report
    );
}

#[test]
fn entity_matching_earliest_tick_across_runs_is_not_hidden_by_reordered_ids() {
    let mut run = 0;
    let report = DeterminismCheck::new(|| {
        run += 1;
        scenario(match run {
            1 => u64::MAX,
            2 => 4,
            _ => 2,
        })
    })
    .ticks(6)
    .runs(3)
    .snapshot_config(capture_config())
    .variant(Variant::MultiThreaded)
    .entity_matching(EntityMatching::ByName)
    .run();
    let DeterminismReport::Diverged(divergence) = report else {
        panic!("expected a health divergence");
    };
    assert_eq!((divergence.run, divergence.tick), (3, 2));
}

#[test]
fn entity_matching_component_struct_key_works_and_reports_round_trip_for_all_matchers() {
    for matching in [
        EntityMatching::ById,
        EntityMatching::ByName,
        EntityMatching::ByComponent(StableKey::type_path().into()),
    ] {
        for diverge_at in [3, u64::MAX] {
            let report = check(matching.clone(), diverge_at);
            if matching != EntityMatching::ById && diverge_at == u64::MAX {
                report.assert_deterministic();
                let json = serde_json::to_value(&report).unwrap();
                assert_eq!(
                    json["Deterministic"]["entity_matching"],
                    serde_json::to_value(&matching).unwrap()
                );
            } else {
                let DeterminismReport::Diverged(divergence) = &report else {
                    panic!("expected divergence with {matching:?}");
                };
                assert_eq!(divergence.parameters.entity_matching, matching);
                assert_eq!(
                    divergence.tick,
                    if matching == EntityMatching::ById {
                        1
                    } else {
                        3
                    }
                );
                if matches!(matching, EntityMatching::ByComponent(_)) {
                    assert!(report.to_string().contains("Component("));
                    assert!(matches!(
                        divergence.matches[0].key,
                        EntityKey::Component { .. }
                    ));
                }
            }
            let json = serde_json::to_string(&report).unwrap();
            assert_eq!(
                serde_json::from_str::<DeterminismReport>(&json).unwrap(),
                report
            );
        }
    }
}

#[test]
fn entity_matching_missing_and_duplicate_keys_are_divergences_not_silent_id_fallbacks() {
    for duplicate in [false, true] {
        let report = DeterminismCheck::new(|| {
            let mut sim = Sim::new(|app| {
                app.register_type::<StableKey>();
            });
            key_test_infrastructure(&mut sim);
            if duplicate {
                sim.world_mut().spawn(Name::new("Same"));
                sim.world_mut().spawn(Name::new("Same"));
            } else {
                sim.world_mut().spawn_empty();
            }
            sim
        })
        .ticks(2)
        .entity_matching(EntityMatching::ByName)
        .run();
        let DeterminismReport::Diverged(divergence) = &report else {
            panic!("invalid keys must fail, even with identical IDs");
        };
        assert_eq!(divergence.tick, 1);
        assert_eq!(divergence.diagnostics.len(), if duplicate { 4 } else { 2 });
        assert!(divergence.matches.iter().all(|m| matches!(&m.key,
            EntityKey::Name { name } if name.starts_with("Infrastructure ")
        )));
        assert!(divergence.diagnostics.iter().all(|d| if duplicate {
            matches!(d.problem, MatchProblem::DuplicateKey { .. })
        } else {
            d.problem == MatchProblem::MissingKey
        }));
        assert!(report.to_string().contains(if duplicate {
            "duplicate key"
        } else {
            "missing key"
        }));
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            serde_json::from_str::<DeterminismReport>(&json).unwrap(),
            report
        );
    }
}

#[cfg(feature = "track_location")]
#[test]
fn entity_matching_location_hint_uses_the_candidate_id_not_the_reference_id() {
    let report = check(EntityMatching::ByName, 3);
    let DeterminismReport::Diverged(divergence) = report else {
        panic!("expected divergence");
    };
    let child = divergence
        .matches
        .iter()
        .find(|m| {
            m.key
                == EntityKey::Name {
                    name: "Child".into(),
                }
        })
        .unwrap();
    let hint = divergence
        .hints
        .change_locations
        .iter()
        .find(|h| h.component == Health::type_path())
        .unwrap();
    assert_eq!(hint.entity, child.after.map(|id| id.to_string()));
    let mutation_line = include_str!("matching.rs")
        .lines()
        .position(|line| line.trim() == "health.value += 1.0;")
        .unwrap()
        + 1;
    assert!(
        hint.location
            .replace('\\', "/")
            .contains(&format!("tests/matching.rs:{mutation_line}:")),
        "{hint:?}"
    );
}

#[test]
fn entity_matching_old_success_json_defaults_to_id_matching() {
    let report: DeterminismReport = serde_json::from_value(json!({
        "Deterministic": { "runs": 2, "ticks": 6 }
    }))
    .unwrap();
    assert_eq!(
        report,
        DeterminismReport::Deterministic {
            runs: 2,
            ticks: 6,
            entity_matching: EntityMatching::ById,
        }
    );
}
