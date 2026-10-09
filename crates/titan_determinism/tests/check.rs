//! Integration coverage for bounded replay, actionable reports, and diagnostics.

use std::{
    collections::HashMap,
    hash::{BuildHasher, Hasher},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput};
use bevy_reflect::{Reflect, TypePath};
use serde_json::json;
use titan_determinism::{
    DeterminismCheck, DeterminismReport, Divergence, SnapshotSettings, Variant,
};
use titan_snapshot::{DiffConfig, SnapshotConfig, TypeFilter};
use titan_test::{InputAction, InputButton, InputScript, ScriptEvent, Sim};

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
struct Position {
    x: f64,
}

fn advance(mut positions: Query<&mut Position>) {
    for mut position in &mut positions {
        position.x += 1.0;
    }
}

fn deterministic_sim() -> Sim {
    Sim::new(|app| {
        app.register_type::<Position>();
        app.world_mut()
            .spawn((Name::new("Player"), Position::default()));
        app.add_systems(Update, advance);
    })
    .with_seed(42)
}

fn positions_only() -> SnapshotConfig {
    SnapshotConfig {
        components: TypeFilter::only([Position::type_path().into()]),
        resources: TypeFilter::only([]),
        ..Default::default()
    }
}

fn diverged(report: DeterminismReport) -> Divergence {
    let DeterminismReport::Diverged(divergence) = report else {
        panic!("expected a divergence, got {report:?}");
    };
    divergence
}

#[test]
fn deterministic_game_passes_both_variants_with_fresh_worlds() {
    for variant in [Variant::Repeat, Variant::MultiThreaded] {
        let mut constructions = 0;
        let report = DeterminismCheck::new(|| {
            constructions += 1;
            let sim = deterministic_sim();
            assert_eq!(sim.current_tick(), 0);
            sim
        })
        .ticks(8)
        .runs(3)
        .variant(variant)
        .run();
        assert_eq!(constructions, 3);
        assert_eq!(
            report,
            DeterminismReport::Deterministic { runs: 3, ticks: 8 }
        );
        report.assert_deterministic();
    }
}

#[test]
fn repeat_and_two_runs_are_the_defaults() {
    assert_eq!(
        DeterminismCheck::new(deterministic_sim).ticks(3).run(),
        DeterminismReport::Deterministic { runs: 2, ticks: 3 }
    );
}

// Engineer two hash-table layouts, rather than hoping RandomState happens to
// produce different iteration orders during a test. This is still the same bug:
// gameplay chooses an action by HashMap iteration instead of an explicit order.
#[derive(Clone, Copy)]
struct LayoutHasher(u64);

impl Hasher for LayoutHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*byte);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 ^= value;
    }
}

#[derive(Clone, Copy)]
struct Layout(u64);

impl BuildHasher for Layout {
    type Hasher = LayoutHasher;

    fn build_hasher(&self) -> Self::Hasher {
        LayoutHasher(self.0)
    }
}

type Actions = HashMap<u64, u32, Layout>;

fn actions(layout: u64) -> Actions {
    let mut map = HashMap::with_capacity_and_hasher(16, Layout(layout));
    map.insert(0, 10);
    map.insert(1, 20);
    map
}

fn opposite_layouts() -> (Actions, Actions) {
    let first = actions(0);
    let first_action = *first.values().next().unwrap();
    let second = (1..64)
        .map(actions)
        .find(|map| *map.values().next().unwrap() != first_action)
        .expect("fixed hash layouts must exercise both action orders");
    (first, second)
}

#[derive(Resource)]
struct UnorderedActions(Actions);

fn take_first_action(actions: Res<UnorderedActions>, mut positions: Query<&mut Position>) {
    let step = f64::from(*actions.0.values().next().unwrap());
    for mut position in &mut positions {
        position.x += step;
    }
}

fn map_order_report() -> DeterminismReport {
    let (first, second) = opposite_layouts();
    let mut maps = [first, second].into_iter();
    DeterminismCheck::new(|| {
        let map = maps.next().expect("only two runs should be constructed");
        Sim::new(|app| {
            app.register_type::<Position>();
            app.insert_resource(UnorderedActions(map));
            app.world_mut()
                .spawn((Name::new("Enemy"), Position::default()));
            app.add_systems(Update, take_first_action);
        })
        .with_seed(42)
    })
    .ticks(5)
    .snapshot_config(positions_only())
    .run()
}

#[test]
fn hash_map_order_bug_names_the_entity_component_and_field() {
    let report = map_order_report();
    let text = report.to_string();
    let divergence = diverged(report);
    assert_eq!(divergence.run, 2);
    assert_eq!(divergence.tick, 1);
    assert_eq!(divergence.diff.entities.len(), 1);
    let entity = &divergence.diff.entities[0];
    assert_eq!(entity.before_name.as_deref(), Some("Enemy"));
    assert_eq!(entity.after_name.as_deref(), Some("Enemy"));
    assert_eq!(entity.components.len(), 1);
    let component = &entity.components[0];
    assert_eq!(component.name, Position::type_path());
    assert_eq!(component.fields.len(), 1);
    assert_eq!(component.fields[0].path, "$.x");
    assert_ne!(component.fields[0].before, component.fields[0].after);
    assert!(divergence.diff.resources.is_empty());
    for needle in [
        "Enemy",
        Position::type_path(),
        "x",
        &entity.entity.to_string(),
    ] {
        assert!(text.contains(needle), "missing {needle:?} in {text}");
    }
    assert_eq!(divergence.parameters.seed, Some(42));
    assert_eq!(divergence.parameters.reference_seed, Some(42));
    assert_eq!(divergence.parameters.ticks, 5);
    assert_eq!(divergence.parameters.runs, 2);
    assert_eq!(divergence.parameters.variant, Variant::Repeat);
    assert_eq!(
        divergence.parameters.snapshot_config,
        SnapshotSettings::from(&positions_only())
    );
    assert_eq!(divergence.parameters.diff_config, DiffConfig::default());
    assert_eq!(divergence.parameters.script, None);
}

fn double_position(mut positions: Query<&mut Position>) {
    for mut position in &mut positions {
        position.x *= 2.0;
    }
}

fn increment_position(mut positions: Query<&mut Position>) {
    for mut position in &mut positions {
        position.x += 1.0;
    }
}

#[test]
fn multithreaded_divergence_hints_name_the_actual_conflicting_system_pair() {
    let mut run = 0;
    let divergence = diverged(
        DeterminismCheck::new(|| {
            run += 1;
            Sim::new(|app| {
                app.register_type::<Position>();
                // Deliberately change the factory's initial state, guaranteeing
                // divergence whichever order the executor chooses. This test
                // covers MT replay and hint integration, not random reordering.
                app.world_mut().spawn((
                    Name::new("Enemy"),
                    Position {
                        x: if run == 1 { 0.0 } else { 100.0 },
                    },
                ));
                // These operations do not commute, and no ordering is declared.
                app.add_systems(Update, (double_position, increment_position));
            })
        })
        .ticks(4)
        .variant(Variant::MultiThreaded)
        .snapshot_config(positions_only())
        .run(),
    );
    assert_eq!(divergence.tick, 1);
    assert_eq!(divergence.parameters.variant, Variant::MultiThreaded);
    let pair = divergence
        .hints
        .ambiguities
        .iter()
        .find(|hint| {
            hint.systems
                .iter()
                .any(|s| s.ends_with("::double_position"))
                && hint
                    .systems
                    .iter()
                    .any(|s| s.ends_with("::increment_position"))
        })
        .expect("hints must identify the two actual ambiguous game systems");
    assert!(pair.schedule.contains("Update"), "{pair:?}");
    assert!(pair.types.iter().any(|name| name == Position::type_path()));
}

#[test]
fn both_report_variants_round_trip_through_json() {
    for report in [
        DeterminismCheck::new(deterministic_sim).ticks(3).run(),
        map_order_report(),
    ] {
        let encoded = serde_json::to_string_pretty(&report).unwrap();
        let decoded: DeterminismReport = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, report);
        assert_eq!(decoded.to_string(), report.to_string());
    }
}

#[test]
fn assert_deterministic_panics_with_the_readable_report() {
    let report = map_order_report();
    let expected = report.to_string();
    let payload = catch_unwind(AssertUnwindSafe(|| report.assert_deterministic()))
        .expect_err("a divergent report must fail the assertion");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("assertion panic should have a readable message");
    assert!(message.contains(&expected), "{message}");
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InputFrame {
    held: bool,
    pressed: bool,
    released: bool,
    mouse_held: bool,
    mouse_pressed: bool,
    mouse_released: bool,
}

type History = Arc<Mutex<Vec<InputFrame>>>;

fn input_sim(history: History) -> Sim {
    Sim::new(|app| {
        app.add_systems(
            Update,
            move |keys: Res<ButtonInput<KeyCode>>, mouse: Res<ButtonInput<MouseButton>>| {
                history.lock().unwrap().push(InputFrame {
                    held: keys.pressed(KeyCode::Space),
                    pressed: keys.just_pressed(KeyCode::Space),
                    released: keys.just_released(KeyCode::Space),
                    mouse_held: mouse.pressed(MouseButton::Left),
                    mouse_pressed: mouse.just_pressed(MouseButton::Left),
                    mouse_released: mouse.just_released(MouseButton::Left),
                });
            },
        );
    })
}

fn script() -> InputScript {
    InputScript {
        version: 1,
        // Unsorted input exercises tick sorting, equal-tick order and automatic
        // tap release when run_script is resumed one update at a time.
        events: vec![
            ScriptEvent {
                tick: 3,
                action: InputAction::Tap(InputButton::Mouse(MouseButton::Left)),
            },
            ScriptEvent {
                tick: 0,
                action: InputAction::Press(InputButton::Key(KeyCode::Space)),
            },
            ScriptEvent {
                tick: 2,
                action: InputAction::Release(InputButton::Key(KeyCode::Space)),
            },
            ScriptEvent {
                tick: 2,
                action: InputAction::Tap(InputButton::Key(KeyCode::Space)),
            },
        ],
    }
}

#[test]
fn script_is_applied_identically_to_every_run_using_sim_playback_semantics() {
    let script = script();
    let expected = History::default();
    input_sim(Arc::clone(&expected)).run_script(&script, 6);
    let expected = expected.lock().unwrap().clone();
    assert!(expected[0].pressed);
    assert!(expected[1].held && !expected[1].pressed);
    assert!(expected[2].held && expected[2].pressed && expected[2].released);
    assert!(expected[3].released && expected[3].mouse_pressed);
    assert!(expected[4].mouse_released);

    for variant in [Variant::Repeat, Variant::MultiThreaded] {
        let mut histories = Vec::new();
        let report = DeterminismCheck::new(|| {
            let history = History::default();
            histories.push(Arc::clone(&history));
            input_sim(history)
        })
        .ticks(6)
        .runs(3)
        .variant(variant)
        .script(script.clone())
        .run();
        report.assert_deterministic();
        assert_eq!(histories.len(), 3);
        for history in histories {
            assert_eq!(*history.lock().unwrap(), expected);
        }
    }
}

fn delayed_sim(diverge_at: u64, updates: Arc<AtomicU64>) -> Sim {
    Sim::new(|app| {
        app.register_type::<Position>();
        app.world_mut()
            .spawn((Name::new("Player"), Position::default()));
        app.add_systems(Update, move |mut positions: Query<&mut Position>| {
            let tick = updates.fetch_add(1, Ordering::SeqCst) + 1;
            for mut position in &mut positions {
                // Re-converge after the chosen tick to catch final-only checks.
                position.x = if tick == diverge_at { 1.0 } else { 0.0 };
            }
        });
    })
}

#[test]
fn captures_every_tick_and_stops_the_candidate_at_its_first_divergence() {
    let mut counters = Vec::new();
    let divergence = diverged(
        DeterminismCheck::new(|| {
            let updates = Arc::new(AtomicU64::new(0));
            let diverge_at = if counters.is_empty() { u64::MAX } else { 3 };
            counters.push(Arc::clone(&updates));
            delayed_sim(diverge_at, updates)
        })
        .ticks(8)
        .snapshot_config(positions_only())
        .run(),
    );
    assert_eq!(divergence.run, 2);
    assert_eq!(divergence.tick, 3);
    assert_eq!(counters.len(), 2);
    assert_eq!(counters[1].load(Ordering::SeqCst), 3);
    let field = &divergence.diff.entities[0].components[0].fields[0];
    assert_eq!(field.before, Some(json!(0.0)));
    assert_eq!(field.after, Some(json!(1.0)));
}

#[test]
fn reports_the_earliest_tick_across_runs_not_the_first_run_examined() {
    let mut run = 0;
    let divergence = diverged(
        DeterminismCheck::new(|| {
            run += 1;
            let tick = match run {
                1 => u64::MAX,
                2 => 4,
                3 => 2,
                _ => panic!("unexpected factory invocation"),
            };
            delayed_sim(tick, Arc::new(AtomicU64::new(0)))
        })
        .ticks(6)
        .runs(3)
        .snapshot_config(positions_only())
        .run(),
    );
    assert_eq!(run, 3);
    assert_eq!(divergence.run, 3);
    assert_eq!(divergence.tick, 2);
}

#[test]
fn tick_one_divergence_stops_without_constructing_later_runs() {
    let mut run = 0;
    let divergence = diverged(
        DeterminismCheck::new(|| {
            run += 1;
            let tick = match run {
                1 => u64::MAX,
                2 => 1,
                _ => panic!("nothing can diverge earlier than completed tick one"),
            };
            delayed_sim(tick, Arc::new(AtomicU64::new(0)))
        })
        .ticks(6)
        .runs(4)
        .snapshot_config(positions_only())
        .run(),
    );
    assert_eq!(run, 2);
    assert_eq!((divergence.run, divergence.tick), (2, 1));
    assert_eq!(divergence.parameters.runs, 4);
}

#[test]
fn tied_divergences_choose_the_lowest_one_based_run_index() {
    let mut run = 0;
    let divergence = diverged(
        DeterminismCheck::new(|| {
            run += 1;
            delayed_sim(
                if run == 1 { u64::MAX } else { 2 },
                Arc::new(AtomicU64::new(0)),
            )
        })
        .ticks(4)
        .runs(3)
        .snapshot_config(positions_only())
        .run(),
    );
    assert_eq!((divergence.run, divergence.tick), (2, 2));
    assert_eq!(run, 3);
    assert_eq!(divergence.parameters.seed, None);
    assert_eq!(divergence.parameters.reference_seed, None);
}

#[test]
fn agreeing_candidates_do_not_prevent_a_later_run_from_being_checked() {
    let mut run = 0;
    let divergence = diverged(
        DeterminismCheck::new(|| {
            run += 1;
            delayed_sim(
                if run == 3 { 3 } else { u64::MAX },
                Arc::new(AtomicU64::new(0)),
            )
        })
        .ticks(5)
        .runs(4)
        .snapshot_config(positions_only())
        .run(),
    );
    assert_eq!(run, 4);
    assert_eq!((divergence.run, divergence.tick), (3, 3));
}

#[test]
fn scenario_parameters_preserve_script_configs_and_both_seeds() {
    let script = script();
    let config = positions_only();
    let diff_config = DiffConfig {
        float_tolerance: 0.25,
    };
    let mut run = 0;
    let divergence = diverged(
        DeterminismCheck::new(|| {
            run += 1;
            delayed_sim(
                if run == 1 { u64::MAX } else { 2 },
                Arc::new(AtomicU64::new(0)),
            )
            .with_seed(if run == 1 { 42 } else { 99 })
        })
        .ticks(5)
        .runs(3)
        .variant(Variant::MultiThreaded)
        .script(script.clone())
        .snapshot_config(config.clone())
        .diff_config(diff_config)
        .run(),
    );
    assert_eq!(divergence.parameters.reference_seed, Some(42));
    assert_eq!(divergence.parameters.seed, Some(99));
    assert_eq!(divergence.parameters.script, Some(script));
    assert_eq!(
        divergence.parameters.snapshot_config,
        SnapshotSettings::from(&config)
    );
    assert_eq!(divergence.parameters.diff_config, diff_config);
    assert_eq!(divergence.parameters.ticks, 5);
    assert_eq!(divergence.parameters.runs, 3);
    assert_eq!(divergence.parameters.variant, Variant::MultiThreaded);
    let report = DeterminismReport::Diverged(divergence);
    assert_eq!(
        serde_json::from_str::<DeterminismReport>(&serde_json::to_string(&report).unwrap())
            .unwrap(),
        report
    );
}

#[test]
fn snapshot_filters_and_float_tolerance_are_used_for_comparisons() {
    for (filter_position, tolerance) in [(true, 0.0), (false, 0.01)] {
        let mut run = 0;
        let mut config = positions_only();
        if filter_position {
            config.components.deny::<Position>();
        }
        let report = DeterminismCheck::new(|| {
            run += 1;
            Sim::new(|app| {
                app.register_type::<Position>();
                app.world_mut().spawn(Position {
                    x: if run == 1 { 1.0 } else { 1.001 },
                });
            })
        })
        .ticks(2)
        .snapshot_config(config)
        .diff_config(DiffConfig {
            float_tolerance: tolerance,
        })
        .run();
        report.assert_deterministic();
    }
}

#[test]
fn ticks_are_required_and_must_be_positive() {
    assert!(catch_unwind(|| DeterminismCheck::new(deterministic_sim).run()).is_err());
    assert!(catch_unwind(|| DeterminismCheck::new(deterministic_sim).ticks(0).run()).is_err());
}

#[test]
fn at_least_two_runs_are_required() {
    for runs in [0, 1] {
        assert!(catch_unwind(|| {
            DeterminismCheck::new(deterministic_sim)
                .ticks(1)
                .runs(runs)
                .run()
        })
        .is_err());
    }
}

#[test]
fn rejects_an_already_ticked_factory_result_in_any_run() {
    for invalid_run in [1, 2] {
        let mut run = 0;
        assert!(catch_unwind(AssertUnwindSafe(|| {
            DeterminismCheck::new(|| {
                run += 1;
                let mut sim = deterministic_sim();
                if run == invalid_run {
                    sim.tick();
                }
                sim
            })
            .ticks(3)
            .run()
        }))
        .is_err());
    }
}

#[cfg(feature = "track_location")]
#[test]
fn track_location_hint_points_to_the_last_component_mutation() {
    let divergence = diverged(map_order_report());
    let entity = divergence.diff.entities[0].entity.to_string();
    let hint = divergence
        .hints
        .change_locations
        .iter()
        .find(|hint| {
            hint.component == Position::type_path() && hint.entity.as_deref() == Some(&entity)
        })
        .expect("the differing reflected component should carry changed_by information");
    assert!(hint.location.contains("tests/check.rs:"), "{hint:?}");
    // Distinguish the game's mutation from the component's spawn location.
    let mutation_line = include_str!("check.rs")
        .lines()
        .position(|line| line.trim() == "position.x += step;")
        .unwrap()
        + 1;
    assert!(
        hint.location.contains(&format!(":{mutation_line}:")),
        "expected the action mutation on line {mutation_line}: {hint:?}"
    );
}

#[cfg(not(feature = "track_location"))]
#[test]
fn source_location_hints_are_empty_without_the_feature() {
    assert!(diverged(map_order_report())
        .hints
        .change_locations
        .is_empty());
}
