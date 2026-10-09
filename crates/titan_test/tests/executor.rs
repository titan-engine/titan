//! Sequential execution policy and executor opt-out regressions.

use bevy_app::{FixedUpdate, Update};
use bevy_ecs::{
    error::{BevyError, ErrorContext},
    prelude::*,
    schedule::{FixedBitSet, ScheduleLabel, SystemExecutor, SystemSchedule},
};
use titan_test::{ExecutorKind, Sim};

/// An executor that must be replaced by the harness before running a schedule.
struct MustBeReplaced;

impl SystemExecutor for MustBeReplaced {
    fn init(&mut self, _: &SystemSchedule) {
        panic!("the harness did not apply its executor policy");
    }

    fn run(
        &mut self,
        _: &mut SystemSchedule,
        _: &mut World,
        _: Option<&FixedBitSet>,
        _: fn(BevyError, ErrorContext),
    ) {
        panic!("the harness did not apply its executor policy");
    }

    fn set_apply_final_deferred(&mut self, _: bool) {}
}

#[derive(Resource, Default)]
struct Count(u64);

#[test]
fn constructor_replaces_game_executors_and_game_systems_run_on_the_calling_thread() {
    let thread = std::thread::current().id();
    let mut sim = Sim::new(|app| {
        app.init_resource::<Count>()
            .add_systems(Update, move |mut count: ResMut<Count>| {
                assert_eq!(std::thread::current().id(), thread);
                count.0 += 1;
            });
        app.world_mut()
            .resource_mut::<Schedules>()
            .get_mut(Update)
            .unwrap()
            .set_executor(MustBeReplaced);
    });
    sim.run_ticks(4);
    assert_eq!(sim.resource::<Count>().0, 4);
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct GameSchedule;

#[test]
fn newly_added_and_replaced_schedules_receive_the_policy_before_the_next_tick() {
    let mut sim = Sim::new(|app| {
        app.init_resource::<Count>()
            .add_systems(Update, |world: &mut World| world.run_schedule(GameSchedule));
    });
    for expected in [1, 2] {
        let mut schedule = Schedule::new(GameSchedule);
        schedule
            .set_executor(MustBeReplaced)
            .add_systems(|mut count: ResMut<Count>| count.0 += 1);
        sim.world_mut().add_schedule(schedule);
        sim.tick();
        assert_eq!(sim.resource::<Count>().0, expected);
    }
}

#[test]
fn multithreaded_opt_out_can_tick_update_and_fixed_schedules() {
    let mut sim = Sim::new(|app| {
        app.init_resource::<Count>()
            .add_systems(Update, |mut count: ResMut<Count>| count.0 += 1)
            .add_systems(FixedUpdate, |mut count: ResMut<Count>| count.0 += 1);
    })
    .with_executor_kind(ExecutorKind::MultiThreaded);
    sim.run_ticks(4);
    assert_eq!(sim.resource::<Count>().0, 8);
}
