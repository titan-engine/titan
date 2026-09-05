use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::{
    DeferredCommandError, ExecutorPolicy, FixedTime, IntoSystem, SystemError, SystemMetadata,
    World, system::System,
};

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

type ExtractedValue = Box<dyn Any + Send + Sync>;

struct Extractor {
    output_type: TypeId,
    extract: Box<dyn Fn(&World) -> ExtractedValue + Send>,
    latest: Option<ExtractedValue>,
}

#[derive(Default)]
struct Schedule {
    systems: Vec<System>,
}

/// A checked schedule failure. Earlier world changes are not rolled back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    Deferred(DeferredCommandError),
    System {
        system: &'static str,
        error: SystemError,
    },
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deferred(error) => error.fmt(f),
            Self::System { system, error } => write!(f, "system {system}: {error}"),
        }
    }
}
impl std::error::Error for AppError {}

/// Configures an application by installing related resources and systems.
pub trait Plugin {
    fn build(&self, app: &mut App);
}

/// Owns a game world and its schedules independently of any platform runner.
pub struct App {
    world: World,
    schedules: HashMap<TypeId, Schedule>,
    startup_complete: bool,
    errors: Vec<AppError>,
    extractors: Vec<Extractor>,
    executor_policy: ExecutorPolicy,
}

impl Default for App {
    fn default() -> Self {
        let mut world = World::new();
        world.insert_resource(FixedTime::default());
        Self {
            world,
            schedules: HashMap::new(),
            startup_complete: false,
            errors: Vec::new(),
            extractors: Vec::new(),
            executor_policy: ExecutorPolicy::default(),
        }
    }
}

impl App {
    /// Creates an application with a 60 Hz fixed clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets execution policy for all schedules. The default is sequential.
    pub fn set_executor_policy(&mut self, policy: ExecutorPolicy) -> &mut Self {
        self.executor_policy = policy;
        self
    }

    /// Returns the configured policy, including on targets using its fallback.
    pub const fn executor_policy(&self) -> ExecutorPolicy {
        self.executor_policy
    }

    /// Returns the application's world.
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// Returns the application's world for direct setup or testing.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Adds a system in insertion order, rejecting conflicting declarations.
    ///
    /// Functions accept typed parameters or exclusive `&mut World` access.
    /// Exclusive closures must annotate their argument as `|world: &mut World|`.
    /// Panics on invalid declarations; use `try_add_systems` for a checked result.
    pub fn add_systems<L, S, Marker>(&mut self, label: L, system: S) -> &mut Self
    where
        L: ScheduleLabel,
        S: IntoSystem<Marker>,
    {
        self.try_add_systems(label, system)
            .expect("invalid system access declaration")
    }

    /// Registers a system after validating all component and resource accesses.
    pub fn try_add_systems<L, S, Marker>(
        &mut self,
        _label: L,
        system: S,
    ) -> Result<&mut Self, SystemError>
    where
        L: ScheduleLabel,
        S: IntoSystem<Marker>,
    {
        let system = system.into_system()?;
        self.schedules
            .entry(TypeId::of::<L>())
            .or_default()
            .systems
            .push(system);
        Ok(self)
    }

    /// Reports systems and explicit deferred boundaries in execution order.
    pub fn system_metadata<L: ScheduleLabel>(
        &self,
        _label: L,
    ) -> impl Iterator<Item = &SystemMetadata> {
        self.schedules
            .get(&TypeId::of::<L>())
            .into_iter()
            .flat_map(|schedule| schedule.systems.iter().map(System::metadata))
    }

    /// Registers an immutable snapshot builder outside the simulation schedules.
    ///
    /// Builders run in registration order after startup, completed fixed ticks,
    /// ordinary schedules, and explicit deferred-command safe points. They receive
    /// shared world access and cannot mutate ECS state or read other snapshots.
    /// Use deterministic builders to preserve deterministic extracted output.
    ///
    /// One builder is retained per output type. Registering that type again
    /// replaces its builder in the original position and invalidates its snapshot.
    /// Registration does not run startup or extraction; the first snapshot appears
    /// at the next extraction boundary after startup has completed.
    pub fn add_extractor<T: Send + Sync + 'static>(
        &mut self,
        extractor: impl Fn(&World) -> T + Send + 'static,
    ) -> &mut Self {
        let registered = Extractor {
            output_type: TypeId::of::<T>(),
            extract: Box::new(move |world: &crate::World| Box::new(extractor(world))),
            latest: None,
        };
        if let Some(existing) = self
            .extractors
            .iter_mut()
            .find(|entry| entry.output_type == registered.output_type)
        {
            *existing = registered;
        } else {
            self.extractors.push(registered);
        }
        self
    }

    /// Reads the latest immutable snapshot without exposing simulation state.
    /// Returns `None` until this type's builder runs after registration.
    pub fn extracted<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extractors
            .iter()
            .find(|entry| entry.output_type == TypeId::of::<T>())?
            .latest
            .as_ref()?
            .downcast_ref()
    }

    /// Refreshes snapshots from the current world, without running systems,
    /// applying deferred commands, or advancing time. Does nothing before startup.
    /// Call this after direct world edits; use `apply_deferred` for queued edits.
    pub fn refresh_extracted(&mut self) {
        if !self.startup_complete {
            return;
        }
        for extractor in &mut self.extractors {
            extractor.latest = Some((extractor.extract)(&self.world));
        }
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

    /// Takes failures produced by deferred commands during prior schedules.
    pub fn take_deferred_errors(&mut self) -> Vec<DeferredCommandError> {
        let mut deferred = Vec::new();
        self.errors.retain(|error| {
            if let AppError::Deferred(error) = error {
                deferred.push(*error);
                false
            } else {
                true
            }
        });
        deferred
    }

    /// Takes typed-system failures while preserving pending deferred failures.
    pub fn take_system_errors(&mut self) -> Vec<AppError> {
        let mut systems = Vec::new();
        self.errors.retain(|error| {
            if matches!(error, AppError::System { .. }) {
                systems.push(error.clone());
                false
            } else {
                true
            }
        });
        systems
    }

    /// Applies queued structural changes at an explicit safe point and reports
    /// all outstanding schedule failures. Successful changes are not rolled back.
    pub fn apply_deferred(&mut self) -> Result<(), Vec<AppError>> {
        self.errors.extend(
            self.world
                .apply_deferred()
                .into_iter()
                .map(AppError::Deferred),
        );
        self.refresh_extracted();
        self.check_errors()
    }

    /// Advances fixed ticks, stopping at the first failed system or deferred boundary.
    ///
    /// Outstanding errors are returned before running startup or any ticks.
    /// A failed fixed update still counts as an executed tick: systems and
    /// successful structural changes are not rolled back. Startup failures
    /// stop execution before the first tick. Errors are consumed by this call.
    pub fn try_advance_fixed(&mut self, ticks: u64) -> Result<(), Vec<AppError>> {
        self.check_errors()?;
        self.run_startup();
        self.check_errors()?;
        for _ in 0..ticks {
            self.advance_fixed(1);
            self.check_errors()?;
        }
        Ok(())
    }

    fn check_errors(&mut self) -> Result<(), Vec<AppError>> {
        let errors = std::mem::take(&mut self.errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
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
        self.refresh_extracted();
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
            self.refresh_extracted();
        }
    }

    fn run_startup(&mut self) {
        if !self.startup_complete {
            self.startup_complete = true;
            self.run_schedule(TypeId::of::<Startup>());
            self.refresh_extracted();
        }
    }

    fn run_schedule(&mut self, label: TypeId) {
        if let Some(schedule) = self.schedules.get_mut(&label) {
            let limit = self.executor_policy.concurrency();
            let mut index = 0;
            while index < schedule.systems.len() {
                if limit > 1 {
                    let mut end = index;
                    while end < schedule.systems.len() && end - index < limit {
                        let candidate = &schedule.systems[end];
                        if !candidate.is_parallel_candidate()
                            || schedule.systems[index..end].iter().any(|prior| {
                                !crate::ecs::access::compatible(
                                    &prior.metadata().accesses,
                                    &candidate.metadata().accesses,
                                )
                            })
                            || crate::ecs::access::check_resources(
                                &self.world,
                                &candidate.metadata().accesses,
                            )
                            .is_err()
                        {
                            break;
                        }
                        end += 1;
                    }
                    if end - index > 1 {
                        let batch = &mut schedule.systems[index..end];
                        let accesses: Vec<_> = batch
                            .iter()
                            .map(|system| system.metadata().accesses.as_slice())
                            .collect();
                        let contexts = crate::ecs::access::SystemContext::prepare_batch(
                            &mut self.world,
                            &accesses,
                        );
                        std::thread::scope(|scope| {
                            let handles: Vec<_> = batch
                                .iter_mut()
                                .zip(contexts)
                                .map(|(system, context)| {
                                    scope.spawn(move || system.run_prepared(context))
                                })
                                .collect();
                            let mut panic = None;
                            for handle in handles {
                                if let Err(payload) = handle.join()
                                    && panic.is_none()
                                {
                                    panic = Some(payload);
                                }
                            }
                            if let Some(payload) = panic {
                                std::panic::resume_unwind(payload);
                            }
                        });
                        index = end;
                        continue;
                    }
                }
                let system = &mut schedule.systems[index];
                if system.is_deferred_boundary() {
                    let failures = self.world.apply_deferred();
                    if !failures.is_empty() {
                        self.errors
                            .extend(failures.into_iter().map(AppError::Deferred));
                        break;
                    }
                } else if let Err(error) = system.run(&mut self.world) {
                    self.errors.push(AppError::System {
                        system: system.metadata().name,
                        error,
                    });
                    break;
                }
                index += 1;
            }
        }
        self.errors.extend(
            self.world
                .apply_deferred()
                .into_iter()
                .map(AppError::Deferred),
        );
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
        app.add_systems(Startup, |world: &mut crate::World| {
            world.resource_mut::<Counts>().unwrap().startup += 1;
        });
        app.add_systems(CustomSchedule, |world: &mut crate::World| {
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
            app.add_systems(Startup, |world: &mut crate::World| {
                let entity = world.spawn();
                world.insert(entity, Position(0)).unwrap();
            });
            app.add_systems(FixedUpdate, |world: &mut crate::World| {
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
                app.add_systems(FixedUpdate, |world: &mut crate::World| {
                    world.resource_mut::<Counts>().unwrap().custom += 1;
                });
            }
        }

        let mut app = App::new();
        app.world_mut().insert_resource(Counts::default());
        app.add_plugin(MovementPlugin).advance_fixed(2);

        assert_eq!(app.world().resource::<Counts>().unwrap().custom, 2);
    }

    #[test]
    fn structural_commands_apply_at_schedule_boundaries() {
        let mut app = App::new();
        app.add_systems(FixedUpdate, |world: &mut crate::World| {
            let mut commands = world.commands();
            commands.spawn_with(Position(10));
        });

        app.advance_fixed(1);

        assert_eq!(app.world().entity_count(), 1);
        assert_eq!(app.world().iter::<Position>().next().unwrap().1.0, 10);
        assert!(app.take_deferred_errors().is_empty());
    }

    #[test]
    fn checked_stepping_stops_after_the_first_failed_tick() {
        let mut app = App::new();
        let entity = app.world_mut().spawn();
        app.add_systems(FixedUpdate, move |world: &mut crate::World| {
            world.commands().despawn(entity).despawn(entity);
        });

        let errors = app.try_advance_fixed(10).unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], super::AppError::Deferred(error) if error.entity() == entity));
        assert_eq!(app.world().resource::<FixedTime>().unwrap().tick(), 1);
        assert!(!app.world().is_alive(entity));
        assert!(app.take_deferred_errors().is_empty());
    }

    #[test]
    fn checked_stepping_reports_startup_and_outstanding_errors_before_ticks() {
        let mut app = App::new();
        let entity = app.world_mut().spawn();
        app.add_systems(Startup, move |world: &mut crate::World| {
            world.commands().despawn(entity).despawn(entity);
        });
        assert_eq!(app.try_advance_fixed(2).unwrap_err().len(), 1);
        assert_eq!(app.world().resource::<FixedTime>().unwrap().tick(), 0);
        app.world_mut().commands().despawn(entity);
        app.update();
        assert_eq!(app.try_advance_fixed(2).unwrap_err().len(), 1);
        assert_eq!(app.world().resource::<FixedTime>().unwrap().tick(), 0);
        app.try_advance_fixed(2).unwrap();
        assert_eq!(app.world().resource::<FixedTime>().unwrap().tick(), 2);
    }

    #[test]
    fn explicit_safe_point_applies_commands_and_returns_failures() {
        let mut app = App::new();
        let entity = app.world_mut().commands().spawn_with(Position(7));
        app.apply_deferred().unwrap();
        assert_eq!(app.world().get::<Position>(entity), Some(&Position(7)));
        app.world_mut().commands().despawn(entity).despawn(entity);
        assert_eq!(app.apply_deferred().unwrap_err().len(), 1);
        assert!(app.apply_deferred().is_ok());
    }
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        tick: u64,
        positions: Vec<i64>,
    }

    fn snapshot(world: &crate::World) -> Snapshot {
        Snapshot {
            tick: world.resource::<FixedTime>().unwrap().tick(),
            positions: world
                .iter::<Position>()
                .map(|(_, position)| position.0)
                .collect(),
        }
    }

    #[test]
    fn extraction_observes_completed_ticks_and_applied_deferred_entities() {
        let mut app = App::new();
        app.add_extractor(snapshot);
        app.add_systems(Startup, |world: &mut crate::World| {
            world.commands().spawn_with(Position(1));
        });
        app.add_systems(FixedUpdate, |world: &mut crate::World| {
            let previous = world.entities().collect::<Vec<_>>();
            let mut commands = world.commands();
            for entity in previous {
                commands.despawn(entity);
            }
            commands.spawn_with(Position(2));
        });
        assert!(app.extracted::<Snapshot>().is_none());
        app.update_schedule(Startup);
        assert_eq!(
            app.extracted::<Snapshot>(),
            Some(&Snapshot {
                tick: 0,
                positions: vec![1]
            })
        );
        app.advance_fixed(1);
        assert_eq!(
            app.extracted::<Snapshot>(),
            Some(&Snapshot {
                tick: 1,
                positions: vec![2]
            })
        );
        // Snapshots are stored separately from ECS resources.
        assert!(app.world().resource::<Snapshot>().is_none());
    }

    #[test]
    fn explicit_extraction_and_command_boundaries_do_not_run_systems_or_ticks() {
        let mut app = App::new();
        app.world_mut().insert_resource(Counts::default());
        app.add_systems(Startup, |world: &mut crate::World| {
            world.resource_mut::<Counts>().unwrap().startup += 1;
        });
        app.add_systems(FixedUpdate, |world: &mut crate::World| {
            world.resource_mut::<Counts>().unwrap().custom += 1;
        });
        app.add_extractor(snapshot);
        app.refresh_extracted();
        app.apply_deferred().unwrap();
        assert!(app.extracted::<Snapshot>().is_none());
        assert_eq!(app.world().resource::<Counts>().unwrap().startup, 0);
        app.update_schedule(Startup);
        app.world_mut().commands().spawn_with(Position(7));
        app.refresh_extracted();
        assert!(app.extracted::<Snapshot>().unwrap().positions.is_empty());
        app.apply_deferred().unwrap();
        assert_eq!(app.extracted::<Snapshot>().unwrap().positions, [7]);
        let entity = app.world().entities().next().unwrap();
        app.world_mut().get_mut::<Position>(entity).unwrap().0 = 9;
        app.refresh_extracted();
        assert_eq!(app.extracted::<Snapshot>().unwrap().positions, [9]);
        assert_eq!(app.extracted::<Snapshot>().unwrap().tick, 0);
        let counts = app.world().resource::<Counts>().unwrap();
        assert_eq!((counts.startup, counts.custom), (1, 0));
    }

    #[test]
    fn extraction_order_and_replacement_are_deterministic() {
        use std::sync::{Arc, Mutex};
        fn run() -> Vec<&'static str> {
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut app = App::new();
            let first = order.clone();
            app.add_extractor(move |_| {
                first.lock().unwrap().push("old");
                1_u32
            });
            let second = order.clone();
            app.add_extractor(move |_| {
                second.lock().unwrap().push("second");
                2_u64
            });
            app.update_schedule(Startup);
            assert_eq!(app.extracted::<u32>(), Some(&1));
            let replacement = order.clone();
            app.add_extractor(move |_| {
                replacement.lock().unwrap().push("replacement");
                3_u32
            });
            assert!(app.extracted::<u32>().is_none());
            app.advance_fixed(2);
            assert_eq!(app.extracted::<u32>(), Some(&3));
            assert_eq!(app.extracted::<u64>(), Some(&2));
            order.lock().unwrap().clone()
        }
        let expected = [
            "old",
            "second",
            "replacement",
            "second",
            "replacement",
            "second",
        ];
        assert_eq!(run(), expected);
        assert_eq!(run(), expected);
    }

    #[test]
    fn failed_deferred_operations_still_extract_the_observed_world() {
        let mut app = App::new();
        let entity = app.world_mut().spawn();
        app.world_mut().insert(entity, Position(1)).unwrap();
        app.add_extractor(snapshot);
        app.add_systems(FixedUpdate, move |world: &mut crate::World| {
            world.commands().despawn(entity).despawn(entity);
        });
        assert!(app.try_advance_fixed(3).is_err());
        assert_eq!(
            app.extracted::<Snapshot>(),
            Some(&Snapshot {
                tick: 1,
                positions: vec![]
            })
        );
        app.world_mut().commands().spawn_with(Position(5));
        app.world_mut().commands().despawn(entity);
        assert!(app.apply_deferred().is_err());
        assert_eq!(
            app.extracted::<Snapshot>(),
            Some(&Snapshot {
                tick: 1,
                positions: vec![5]
            })
        );
    }
}
