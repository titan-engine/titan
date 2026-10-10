//! Configure Bevy's public seeded topological shuffler without eagerly building
//! schedules or resetting executor bookkeeping at every tick boundary.

use bevy_app::Main;
use bevy_ecs::{
    schedule::{IntoScheduleConfigs, Schedules, SingleThreadedExecutor, SystemSet},
    world::World,
};

/// An empty set used solely to request a build via the public API.
/// Changing build settings alone does not mark an initialized schedule dirty.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Rebuild;

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
    for (_, schedule) in schedules.iter_mut() {
        let mut settings = schedule.get_build_settings();
        if settings.shuffle_seed == Some(seed) {
            continue;
        }
        settings.shuffle_seed = Some(seed);
        schedule.set_build_settings(settings);
        schedule.set_executor(SingleThreadedExecutor::new());
        // Build lazily at the schedule's normal first/next execution. Locals
        // may depend on startup or state-entry resources. Once configured, an
        // unchanged schedule must not rebuild: executor.init() clears its
        // bookkeeping for deferred buffers left pending across ticks.
        schedule.configure_sets(Rebuild);
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
