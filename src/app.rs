use std::any::TypeId;
use std::collections::HashMap;

use crate::{FixedTime, World};

/// Identifies an independently runnable collection of systems.
pub trait ScheduleLabel: Send + Sync + 'static {}

/// Systems that initialize a game. This schedule runs exactly once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Startup;
impl ScheduleLabel for Startup {}

/// Systems driven by deterministic fixed time.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedUpdate;
impl ScheduleLabel for FixedUpdate {}

/// Systems that update once per runner iteration.
#[derive(Clone, Copy, Debug, Default)]
pub struct Update;
impl ScheduleLabel for Update {}

type System = Box<dyn FnMut(&mut World) + Send + 'static>;

#[derive(Default)]
struct Schedule {
    systems: Vec<System>,
}

impl Schedule {
    fn run(&mut self, world: &mut World) {
        for system in &mut self.systems {
            system(world);
        }
    }
}

/// Configures an application by installing related resources and systems.
pub trait Plugin {
    fn build(&self, app: &mut App);
}

/// Owns a game world and its schedules independently of any platform runner.
pub struct App {
    world: World,
    schedules: HashMap<TypeId, Schedule>,
    startup_complete: bool,
}

impl Default for App {
    fn default() -> Self {
        let mut world = World::new();
        world.insert_resource(FixedTime::default());
        Self {
            world,
            schedules: HashMap::new(),
            startup_complete: false,
        }
    }
}

impl App {
    /// Creates an application with a 60 Hz fixed clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the application's world.
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// Returns the application's world for direct setup or testing.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Adds a system to a schedule, preserving insertion order.
    pub fn add_systems<L, S>(&mut self, _label: L, system: S) -> &mut Self
    where
        L: ScheduleLabel,
        S: FnMut(&mut World) + Send + 'static,
    {
        self.schedules
            .entry(TypeId::of::<L>())
            .or_default()
            .systems
            .push(Box::new(system));
        self
    }

    /// Applies a plugin to this application.
    pub fn add_plugin<P: Plugin>(&mut self, plugin: P) -> &mut Self {
        plugin.build(self);
        self
    }

    /// Replaces the deterministic fixed clock.
    pub fn set_fixed_time(&mut self, fixed_time: FixedTime) -> &mut Self {
        self.world.insert_resource(fixed_time);
        self
    }

    /// Runs one customizable schedule.
    ///
    /// Running `Startup` explicitly still obeys its run-once guarantee.
    pub fn update_schedule<L: ScheduleLabel>(&mut self, _label: L) {
        if TypeId::of::<L>() == TypeId::of::<Startup>() {
            self.run_startup();
            return;
        }
        self.run_startup();
        self.run_schedule(TypeId::of::<L>());
    }

    /// Runs one ordinary update after ensuring startup has completed.
    pub fn update(&mut self) {
        self.update_schedule(Update);
    }

    /// Advances deterministic simulation by exactly `ticks` fixed updates.
    pub fn advance_fixed(&mut self, ticks: u64) {
        self.run_startup();
        for _ in 0..ticks {
            self.run_schedule(TypeId::of::<FixedUpdate>());
            self.world
                .resource_mut::<FixedTime>()
                .expect("App always contains FixedTime")
                .complete_tick();
        }
    }

    fn run_startup(&mut self) {
        if !self.startup_complete {
            self.startup_complete = true;
            self.run_schedule(TypeId::of::<Startup>());
        }
    }

    fn run_schedule(&mut self, label: TypeId) {
        if let Some(schedule) = self.schedules.get_mut(&label) {
            schedule.run(&mut self.world);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{App, Component, FixedTime, FixedUpdate, Plugin, ScheduleLabel, Startup};

    #[derive(Debug, PartialEq)]
    struct Position(i64);
    impl Component for Position {}

    #[derive(Default)]
    struct Counts {
        startup: usize,
        custom: usize,
    }

    #[derive(Clone, Copy)]
    struct CustomSchedule;
    impl ScheduleLabel for CustomSchedule {}

    #[test]
    fn startup_runs_once_before_other_schedules() {
        let mut app = App::new();
        app.world_mut().insert_resource(Counts::default());
        app.add_systems(Startup, |world| {
            world.resource_mut::<Counts>().unwrap().startup += 1;
        });
        app.add_systems(CustomSchedule, |world| {
            world.resource_mut::<Counts>().unwrap().custom += 1;
        });

        app.update_schedule(CustomSchedule);
        app.update_schedule(CustomSchedule);
        app.update_schedule(Startup);

        let counts = app.world().resource::<Counts>().unwrap();
        assert_eq!(counts.startup, 1);
        assert_eq!(counts.custom, 2);
    }

    #[test]
    fn fixed_updates_are_deterministic_and_do_not_read_wall_time() {
        fn run() -> (i64, FixedTime) {
            let mut app = App::new();
            app.set_fixed_time(FixedTime::from_duration(Duration::from_millis(20)));
            app.add_systems(Startup, |world| {
                let entity = world.spawn();
                world.insert(entity, Position(0)).unwrap();
            });
            app.add_systems(FixedUpdate, |world| {
                for (_, position) in world.iter_mut::<Position>() {
                    position.0 += 3;
                }
            });
            app.advance_fixed(120);

            let position = app.world().iter::<Position>().next().unwrap().1.0;
            let time = *app.world().resource::<FixedTime>().unwrap();
            (position, time)
        }

        assert_eq!(run(), run());
        assert_eq!(run().0, 360);
        assert_eq!(run().1.tick(), 120);
    }

    #[test]
    fn plugins_configure_the_same_app_api() {
        struct MovementPlugin;
        impl Plugin for MovementPlugin {
            fn build(&self, app: &mut App) {
                app.add_systems(FixedUpdate, |world| {
                    world.resource_mut::<Counts>().unwrap().custom += 1;
                });
            }
        }

        let mut app = App::new();
        app.world_mut().insert_resource(Counts::default());
        app.add_plugin(MovementPlugin).advance_fixed(2);

        assert_eq!(app.world().resource::<Counts>().unwrap().custom, 2);
    }
}
