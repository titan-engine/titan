//! Hidden single-threaded order dependencies, reproducible without timing races.

use bevy_app::{First, FixedUpdate, Startup, Update};
use bevy_ecs::{
    prelude::*,
    schedule::{ScheduleLabel, Schedules},
};
use bevy_reflect::Reflect;
use titan_determinism::{DeterminismCheck, DeterminismReport, Variant};
use titan_test::Sim;

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct Decision(u32);

fn first(mut decision: ResMut<Decision>) {
    decision.0 = 1;
}
fn second(mut decision: ResMut<Decision>) {
    decision.0 = 2;
}
fn third(mut decision: ResMut<Decision>) {
    decision.0 = 3;
}

fn scenario(fixed: bool) -> Sim {
    Sim::new(|app| {
        app.register_type::<Decision>().init_resource::<Decision>();
        if fixed {
            app.add_systems(FixedUpdate, (first, second));
        } else {
            app.add_systems(Update, (first, second));
        }
    })
    .with_fixed_dt(1.0 / 60.0)
}

fn divergent_seed(factory: impl Fn() -> Sim) -> (u64, DeterminismReport) {
    for seed in 0..32 {
        let report = DeterminismCheck::new(&factory)
            .ticks(3)
            .variant(Variant::ShuffleAmbiguous { seed })
            .run();
        if matches!(report, DeterminismReport::Diverged(_)) {
            return (seed, report);
        }
    }
    panic!("seeds must explore both orders of the ambiguous writes");
}

#[test]
fn repeat_passes_shuffle_fails_and_reports_pair_and_chosen_order() {
    for fixed in [false, true] {
        let factory = || scenario(fixed);
        DeterminismCheck::new(factory)
            .ticks(3)
            .runs(3)
            .run()
            .assert_deterministic();
        let (seed, report) = divergent_seed(factory);
        let DeterminismReport::Diverged(divergence) = &report else {
            unreachable!()
        };
        assert_eq!(divergence.run, 2);
        assert!(divergence.tick <= 2);
        assert_eq!(
            divergence.parameters.variant,
            Variant::ShuffleAmbiguous { seed }
        );
        let hint = divergence
            .hints
            .ambiguities
            .iter()
            .find(|hint| {
                hint.systems.iter().any(|name| name.ends_with("::first"))
                    && hint.systems.iter().any(|name| name.ends_with("::second"))
            })
            .unwrap_or_else(|| {
                panic!("the added edges must not erase the original ambiguity hint: {report}")
            });
        assert_eq!(hint.schedule, if fixed { "FixedUpdate" } else { "Update" });
        assert_eq!(hint.types, [core::any::type_name::<Decision>()]);
        let order = hint.order.as_ref().unwrap();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, hint.systems);
        assert!(report
            .to_string()
            .contains(&format!("shuffled order: {} -> {}", order[0], order[1])));
        // Same factory, seed and build give identical divergence and hints.
        for _ in 0..3 {
            assert_eq!(
                report,
                DeterminismCheck::new(factory)
                    .ticks(3)
                    .variant(Variant::ShuffleAmbiguous { seed })
                    .run()
            );
        }
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            report,
            serde_json::from_str::<DeterminismReport>(&json).unwrap()
        );
    }
}

#[test]
fn explicit_orders_are_preserved_and_exempt_pairs_are_not_reported() {
    for weak in [false, true] {
        let factory = || {
            Sim::new(|app| {
                app.register_type::<Decision>().init_resource::<Decision>();
                if weak {
                    app.add_systems(Update, (first, second).chain_weak());
                } else {
                    app.add_systems(Update, (first, second).chain());
                }
            })
        };
        for seed in 0..16 {
            DeterminismCheck::new(factory)
                .ticks(2)
                .variant(Variant::ShuffleAmbiguous { seed })
                .run()
                .assert_deterministic();
        }
    }
    // Accepted ambiguities remain exempt from diagnostics, but their systems
    // are still unordered and may execute in either seeded topological order.
    let factory = || {
        Sim::new(|app| {
            app.register_type::<Decision>()
                .init_resource::<Decision>()
                .add_systems(Update, (first, second));
            app.world_mut()
                .resource_mut::<Schedules>()
                .get_mut(Update)
                .unwrap()
                .ignore_ambiguity(first, second);
        })
    };
    let (_, report) = divergent_seed(factory);
    let DeterminismReport::Diverged(divergence) = report else {
        unreachable!()
    };
    assert!(divergence
        .hints
        .ambiguities
        .iter()
        .all(|hint| { !hint.systems.iter().any(|name| name.ends_with("::first")) }));
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct Late;

fn install_late(world: &mut World) {
    let mut schedule = Schedule::new(Late);
    schedule.add_systems((first, second));
    world.resource_mut::<Schedules>().insert(schedule);
}

fn run_late(world: &mut World) {
    world.run_schedule(Late);
}

#[test]
fn discovers_new_and_replaced_schedules_before_later_ticks() {
    for replace in [false, true] {
        let factory = || {
            Sim::new(|app| {
                app.register_type::<Decision>()
                    .init_resource::<Decision>()
                    .add_systems(Startup, install_late)
                    .add_systems(Update, run_late);
                if replace {
                    let mut original = Schedule::new(Late);
                    original.add_systems(third);
                    app.world_mut().resource_mut::<Schedules>().insert(original);
                }
            })
        };
        DeterminismCheck::new(factory)
            .ticks(3)
            .run()
            .assert_deterministic();
        let (_, report) = divergent_seed(factory);
        let DeterminismReport::Diverged(divergence) = report else {
            unreachable!()
        };
        // Installed and immediately run during tick 1: first eligible boundary
        // is tick 2, with no stale cache for a replaced schedule's label.
        assert_eq!(divergence.tick, 2);
        assert!(divergence
            .hints
            .ambiguities
            .iter()
            .any(|hint| hint.schedule == "Late" && hint.order.is_some()));
    }
}

#[derive(Resource)]
struct Ready;

struct Cached;
impl FromWorld for Cached {
    fn from_world(world: &mut World) -> Self {
        assert!(world.contains_resource::<Ready>());
        Cached
    }
}

fn ready(mut commands: Commands) {
    commands.insert_resource(Ready);
}
fn use_cached(_: Local<Cached>) {}

struct DormantCached;
impl FromWorld for DormantCached {
    fn from_world(_: &mut World) -> Self {
        panic!("dormant schedules must not initialize");
    }
}
fn use_dormant(_: Local<DormantCached>) {}

#[test]
fn preserves_startup_dependent_locals_and_dormant_schedule_initialization() {
    let factory = || {
        Sim::new(|app| {
            app.add_systems(Startup, ready)
                .add_systems(Update, use_cached);
            // Never executed: its local must never initialize.
            let mut dormant = Schedule::new(Late);
            dormant.add_systems(use_dormant);
            app.world_mut().resource_mut::<Schedules>().insert(dormant);
        })
    };
    DeterminismCheck::new(factory)
        .ticks(3)
        .run()
        .assert_deterministic();
    for seed in 0..4 {
        DeterminismCheck::new(factory)
            .ticks(3)
            .variant(Variant::ShuffleAmbiguous { seed })
            .run()
            .assert_deterministic();
    }
}

fn enqueue_increment(mut commands: Commands) {
    commands.queue(|world: &mut World| {
        world.resource_mut::<Decision>().0 += 1;
    });
}

#[test]
fn preserves_deferred_buffers_across_ticks_and_new_schedule_discovery() {
    for late in [false, true] {
        let factory = || {
            Sim::new(|app| {
                app.register_type::<Decision>()
                    .init_resource::<Decision>()
                    .add_systems(First, |world: &mut World| {
                        // Sim installs its executor before the first update.
                        // Set this afterward, without replacing the executor.
                        world
                            .resource_mut::<Schedules>()
                            .get_mut(Update)
                            .unwrap()
                            .set_apply_final_deferred(false);
                    })
                    .add_systems(Update, (ApplyDeferred, enqueue_increment).chain());
                if late {
                    app.add_systems(Startup, |world: &mut World| {
                        world
                            .resource_mut::<Schedules>()
                            .insert(Schedule::new(Late));
                    });
                }
            })
        };
        let mut reference = factory();
        reference.tick();
        assert_eq!(reference.world().resource::<Decision>().0, 0);
        reference.tick();
        assert_eq!(reference.world().resource::<Decision>().0, 1);
        for seed in 0..4 {
            DeterminismCheck::new(factory)
                .ticks(3)
                .variant(Variant::ShuffleAmbiguous { seed })
                .run()
                .assert_deterministic();
        }
    }
}

#[test]
fn empty_scenarios_do_not_diverge_from_diagnostic_state() {
    DeterminismCheck::new(|| Sim::new(|_| {}))
        .ticks(3)
        .runs(3)
        .variant(Variant::ShuffleAmbiguous { seed: u64::MAX })
        .run()
        .assert_deterministic();
}
