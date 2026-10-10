//! Configure Bevy's public seeded topological shuffler without eagerly building
//! schedules or resetting executor bookkeeping at every tick boundary.

use bevy_app::Main;
use bevy_ecs::{
    schedule::{IntoScheduleConfigs, Schedule, Schedules, SingleThreadedExecutor, SystemSet},
    world::World,
};

/// Private, per-schedule proof that the harness installed both shuffle settings
/// and a single-threaded executor. Copying public build settings cannot copy it.
/// The empty set also requests a build, which changing settings alone does not.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Configured;

pub(crate) fn is_configured(schedule: &Schedule, seed: u64) -> bool {
    schedule
        .graph()
        .system_sets
        .get_key(Configured.intern())
        .is_some()
        && schedule.get_build_settings().shuffle_seed == Some(seed)
}

/// Install once, before any candidate updates. Configure future schedules from
/// the app's driver instead of borrowing `Sim::world_mut()` every tick: that API
/// invalidates Sim's executor cache and would reset live deferred bookkeeping.
pub(crate) fn install(world: &mut World, seed: u64) {
    configure(world, seed);
    world
        .resource_mut::<Schedules>()
        .get_mut(Main)
        .expect("ShuffleAmbiguous requires Bevy's Main schedule driver")
        .add_systems((move |world: &mut World| configure(world, seed)).before(Main::run_main));
}

pub(crate) fn configure(world: &mut World, seed: u64) {
    let Some(mut schedules) = world.get_resource_mut::<Schedules>() else {
        return;
    };
    // No public API exposes an executor's pending deferred-buffer mask. An
    // already-initialized, unowned schedule may have run and retained Commands;
    // replacing its executor or rebuilding it would destroy that bookkeeping.
    // Validate the entire batch before touching any schedule, and fail explicitly
    // instead of producing an instrumentation-induced divergence or false pass.
    for (label, schedule) in schedules.iter() {
        assert!(
            is_configured(schedule, seed) || schedule.systems().is_err(),
            "ShuffleAmbiguous cannot configure already-initialized schedule {label:?}; \
             make new/replacement schedules available before their first run, and do not \
             override a live schedule's shuffle settings"
        );
    }
    for (_, schedule) in schedules.iter_mut() {
        if is_configured(schedule, seed) {
            continue;
        }
        let mut settings = schedule.get_build_settings();
        settings.shuffle_seed = Some(seed);
        schedule.set_build_settings(settings);
        schedule.set_executor(SingleThreadedExecutor::new());
        // Build lazily at the schedule's normal first/next execution. Locals
        // may depend on startup or state-entry resources. Once configured, an
        // unchanged schedule must not rebuild: executor.init() clears its
        // bookkeeping for deferred buffers left pending across ticks.
        schedule.configure_sets(Configured);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::{prelude::*, schedule::ScheduleLabel};

    #[derive(Resource, Default)]
    struct Trace(Vec<u8>);

    #[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
    struct Test;

    fn a(mut trace: ResMut<Trace>) {
        trace.0.push(1);
    }
    fn b(mut trace: ResMut<Trace>) {
        trace.0.push(2);
    }
    fn c(mut trace: ResMut<Trace>) {
        trace.0.push(3);
    }

    fn sample(seed: u64, constrained: bool) -> Vec<u8> {
        let mut world = World::new();
        world.init_resource::<Trace>();
        world.init_resource::<Schedules>();
        let mut schedule = Schedule::new(Test);
        if constrained {
            schedule.add_systems(((a, b).chain(), c));
        } else {
            schedule.add_systems((a, b, c));
        }
        world.resource_mut::<Schedules>().insert(schedule);
        configure(&mut world, seed);
        world.run_schedule(Test);
        world.resource::<Trace>().0.clone()
    }

    fn enqueue(mut commands: Commands) {
        commands.queue(|world: &mut World| world.resource_mut::<Trace>().0.push(4));
    }

    #[test]
    fn already_run_replacements_fail_before_discarding_deferred_buffers() {
        for copy_seed in [false, true] {
            let mut world = World::new();
            world.init_resource::<Trace>();
            world.init_resource::<Schedules>();
            world
                .resource_mut::<Schedules>()
                .insert(Schedule::new(Test));
            configure(&mut world, 42);
            let settings = world
                .resource::<Schedules>()
                .get(Test)
                .unwrap()
                .get_build_settings();
            let mut replacement = Schedule::new(Test);
            replacement.set_executor(SingleThreadedExecutor::new());
            replacement.set_apply_final_deferred(false);
            replacement.add_systems((ApplyDeferred, enqueue).chain());
            if copy_seed {
                replacement.set_build_settings(settings);
            }
            world.resource_mut::<Schedules>().insert(replacement);
            world.run_schedule(Test);
            assert!(world.resource::<Trace>().0.is_empty());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                configure(&mut world, 42);
            }));
            assert!(
                result.is_err(),
                "late discovery must not reconfigure a live executor"
            );
            // The failed configuration must leave the old executor untouched:
            // tick 2's ApplyDeferred still flushes tick 1's pending command.
            world.run_schedule(Test);
            assert_eq!(world.resource::<Trace>().0, [4]);
        }
    }

    #[test]
    fn copied_seed_does_not_skip_configuring_a_fresh_replacement() {
        #[derive(Resource)]
        struct Thread(std::thread::ThreadId, bool);
        fn on_caller(mut thread: ResMut<Thread>) {
            thread.1 &= std::thread::current().id() == thread.0;
        }
        let mut world = World::new();
        world.insert_resource(Thread(std::thread::current().id(), true));
        world.init_resource::<Schedules>();
        world
            .resource_mut::<Schedules>()
            .insert(Schedule::new(Test));
        configure(&mut world, 42);
        let settings = world
            .resource::<Schedules>()
            .get(Test)
            .unwrap()
            .get_build_settings();
        let mut replacement = Schedule::new(Test);
        replacement.set_build_settings(settings);
        replacement.add_systems(on_caller);
        world.resource_mut::<Schedules>().insert(replacement);
        configure(&mut world, 42);
        world.run_schedule(Test);
        assert!(
            world.resource::<Thread>().1,
            "a copied seed must not retain the default multithreaded executor"
        );
    }

    #[test]
    fn seeded_choices_reproduce_and_three_way_conflicts_cannot_cycle() {
        let mut permutations = alloc::collections::BTreeSet::new();
        for seed in 0..64 {
            let trace = sample(seed, false);
            assert_eq!(trace, sample(seed, false));
            permutations.insert(trace);
        }
        assert_eq!(permutations.len(), 6);
    }

    #[test]
    fn randomization_respects_existing_dependency_paths() {
        for seed in 0..64 {
            let trace = sample(seed, true);
            assert!(trace.iter().position(|&n| n == 1) < trace.iter().position(|&n| n == 2));
            assert_eq!(trace, sample(seed, true));
        }
    }
}
