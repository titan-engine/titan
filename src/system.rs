//! Typed, access-validated systems for application schedules.
use crate::{
    World,
    ecs::{
        AccessMode, AccessTarget, SystemAccess, SystemError, SystemParam,
        access::{SystemContext, validate},
    },
};
use std::marker::PhantomData;

/// Read-only description of a system's declared world access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemMetadata {
    pub name: &'static str,
    pub accesses: Vec<SystemAccess>,
}
type ExclusiveRunner = Box<dyn FnMut(&mut World) + Send>;
type TypedRunner = Box<dyn for<'w> FnMut(SystemContext<'w>) + Send>;
enum Runner {
    Exclusive(ExclusiveRunner),
    Typed(TypedRunner),
}

/// Application execution policy. Sequential execution is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutorPolicy {
    #[default]
    Sequential,
    /// Runs compatible typed systems concurrently on native targets.
    /// The limit includes all executing callbacks. One uses sequential execution;
    /// WebAssembly always falls back to sequential execution.
    /// Callback order and external or interior-mutability effects are unspecified.
    Parallel { max_threads: std::num::NonZeroUsize },
}
impl ExecutorPolicy {
    pub(crate) fn concurrency(self) -> usize {
        match self {
            Self::Sequential => 1,
            Self::Parallel { max_threads } => {
                if cfg!(target_arch = "wasm32") {
                    1
                } else {
                    max_threads.get()
                }
            }
        }
    }
}
/// A registered system or explicit deferred-command boundary.
pub struct System {
    metadata: SystemMetadata,
    runner: Option<Runner>,
}
impl System {
    pub fn metadata(&self) -> &SystemMetadata {
        &self.metadata
    }
    pub(crate) fn is_deferred_boundary(&self) -> bool {
        self.runner.is_none()
    }
    pub(crate) fn run(&mut self, world: &mut World) -> Result<(), SystemError> {
        match self.runner.as_mut().expect("boundary handled by schedule") {
            Runner::Exclusive(runner) => runner(world),
            Runner::Typed(runner) => {
                runner(SystemContext::prepare(world, &self.metadata.accesses)?)
            }
        }
        Ok(())
    }
    pub(crate) fn is_parallel_candidate(&self) -> bool {
        matches!(self.runner, Some(Runner::Typed(_)))
            && self.metadata.accesses.iter().all(|access| {
                matches!(
                    access.target,
                    AccessTarget::Component | AccessTarget::Resource
                )
            })
    }
    pub(crate) fn run_prepared(&mut self, context: SystemContext<'_>) {
        match self.runner.as_mut().expect("typed system") {
            Runner::Typed(runner) => runner(context),
            Runner::Exclusive(_) => unreachable!("exclusive system in parallel batch"),
        }
    }
}
/// Converts a function with zero to four typed parameters, an exclusive
/// `&mut World` function, or [`ApplyDeferred`] into a scheduled system.
pub trait IntoSystem<Marker> {
    fn into_system(self) -> Result<System, SystemError>;
}
#[doc(hidden)]
pub struct Exclusive;
#[doc(hidden)]
pub struct Parameters<P>(PhantomData<fn() -> P>);
#[doc(hidden)]
pub struct DeferredMarker;
/// Apply queued structural changes before subsequent systems in this schedule.
/// Schedules also retain their automatic final deferred-command boundary.
pub struct ApplyDeferred;
impl IntoSystem<DeferredMarker> for ApplyDeferred {
    fn into_system(self) -> Result<System, SystemError> {
        Ok(System {
            metadata: SystemMetadata {
                name: "ApplyDeferred",
                accesses: Vec::new(),
            },
            runner: None,
        })
    }
}
impl<F: FnMut(&mut World) + Send + 'static> IntoSystem<Exclusive> for F {
    fn into_system(mut self) -> Result<System, SystemError> {
        Ok(System {
            metadata: SystemMetadata {
                name: std::any::type_name::<F>(),
                accesses: vec![SystemAccess::typed::<World>(
                    AccessTarget::World,
                    AccessMode::Write,
                )],
            },
            runner: Some(Runner::Exclusive(Box::new(move |world| self(world)))),
        })
    }
}
impl<F: FnMut() + Send + 'static> IntoSystem<Parameters<()>> for F {
    fn into_system(mut self) -> Result<System, SystemError> {
        Ok(System {
            metadata: SystemMetadata {
                name: std::any::type_name::<F>(),
                accesses: Vec::new(),
            },
            runner: Some(Runner::Typed(Box::new(move |_| self()))),
        })
    }
}
macro_rules! function_system {
    ($($param:ident:$value:ident),+) => {
        impl<F, $($param),+> IntoSystem<Parameters<($($param,)+)>> for F
        where
            $($param: SystemParam + 'static,)+
            F: FnMut($($param),+) + for<'w> FnMut($($param::Item<'w>),+) + Send + 'static,
        {
            fn into_system(mut self) -> Result<System, SystemError> {
                let mut accesses = Vec::new();
                $(accesses.extend($param::accesses());)+
                validate(&accesses)?;
                let metadata = SystemMetadata { name: std::any::type_name::<F>(), accesses: accesses.clone() };
                Ok(System { metadata, runner: Some(Runner::Typed(Box::new(move |mut context| {
                    $(let $value = $param::fetch(&mut context);)+
                    self($($value),+);
                }))) })
            }
        }
    };
}
function_system!(A:a);
function_system!(A:a,B:b);
function_system!(A:a,B:b,C:c);
function_system!(A:a,B:b,C:c,D:d);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, AppError, Commands, Component, FixedUpdate, Query, Res, ResMut, Startup};
    #[derive(Component)]
    struct Position(i32);
    #[derive(Component)]
    struct Velocity(i32);
    #[derive(Component)]
    struct Extra(i32);
    #[derive(Component)]
    struct Fourth(i32);
    struct Scale(i32);
    #[derive(Default)]
    struct Total(i32);

    fn movement(
        mut query: Query<(&mut Position, &Velocity)>,
        scale: Res<Scale>,
        mut total: ResMut<Total>,
        mut commands: Commands,
    ) {
        query.for_each(|entity, (position, velocity)| {
            position.0 += velocity.0 * scale.0;
            total.0 += position.0;
            commands.insert(entity, Extra(9));
        });
    }

    fn parallel() -> crate::ExecutorPolicy {
        crate::ExecutorPolicy::Parallel {
            max_threads: std::num::NonZeroUsize::new(2).unwrap(),
        }
    }

    #[test]
    fn execution_policy_is_opt_in_and_one_thread_preserves_order() {
        use std::sync::{Arc, Mutex};
        let mut app = App::new();
        assert_eq!(app.executor_policy(), crate::ExecutorPolicy::Sequential);
        let order = Arc::new(Mutex::new(Vec::new()));
        app.set_executor_policy(crate::ExecutorPolicy::Parallel {
            max_threads: std::num::NonZeroUsize::new(1).unwrap(),
        });
        for value in 0..4 {
            let order = order.clone();
            app.add_systems(FixedUpdate, move || order.lock().unwrap().push(value));
        }
        app.try_advance_fixed(1).unwrap();
        assert_eq!(*order.lock().unwrap(), [0, 1, 2, 3]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_readers_actually_overlap_with_shared_component_and_resource() {
        use std::sync::mpsc;
        use std::time::Duration;
        let mut app = App::new();
        app.set_executor_policy(parallel());
        let entity = app.world_mut().spawn();
        app.world_mut().insert(entity, Position(7)).unwrap();
        app.world_mut().insert_resource(Scale(3));
        let (left_tx, left_rx) = mpsc::channel();
        let (right_tx, right_rx) = mpsc::channel();
        app.add_systems(
            FixedUpdate,
            move |mut query: Query<&Position>, scale: Res<Scale>| {
                query.for_each(|_, position| assert_eq!(position.0 * scale.0, 21));
                left_tx.send(()).unwrap();
                right_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("callbacks did not overlap");
            },
        );
        app.add_systems(
            FixedUpdate,
            move |mut query: Query<&Position>, scale: Res<Scale>| {
                query.for_each(|_, position| assert_eq!(position.0 * scale.0, 21));
                right_tx.send(()).unwrap();
                left_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("callbacks did not overlap");
            },
        );
        app.try_advance_fixed(1).unwrap();
    }

    #[test]
    fn parallel_missing_resource_runs_valid_prefix_and_flushes_earlier_commands() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let mut app = App::new();
        app.set_executor_policy(parallel());
        let count = Arc::new(AtomicUsize::new(0));
        app.add_systems(FixedUpdate, |mut commands: Commands| {
            commands.spawn_with(Position(9));
        });
        for _ in 0..3 {
            let count = count.clone();
            app.add_systems(FixedUpdate, move || {
                count.fetch_add(1, Ordering::SeqCst);
            });
        }
        app.add_systems(FixedUpdate, |_: Res<Scale>| {
            panic!("missing resource callback ran")
        });
        app.add_systems(FixedUpdate, || {
            panic!("callback after missing resource ran")
        });
        assert!(matches!(
            &app.try_advance_fixed(1).unwrap_err()[0],
            AppError::System {
                error: SystemError::MissingResource { .. },
                ..
            }
        ));
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(app.world().iter::<Position>().next().unwrap().1.0, 9);
    }

    #[test]
    fn typed_four_parameter_system_joins_components_resources_and_commands() {
        let mut app = App::new();
        let entity = app.world_mut().spawn();
        app.world_mut().insert(entity, Position(2)).unwrap();
        app.world_mut().insert(entity, Velocity(3)).unwrap();
        app.world_mut().insert_resource(Scale(2));
        app.world_mut().insert_resource(Total::default());
        app.add_systems(FixedUpdate, movement);
        app.try_advance_fixed(1).unwrap();
        assert_eq!(app.world().get::<Position>(entity).unwrap().0, 8);
        assert_eq!(app.world().resource::<Total>().unwrap().0, 8);
        assert_eq!(app.world().get::<Extra>(entity).unwrap().0, 9);
        let metadata = app.system_metadata(FixedUpdate).next().unwrap();
        assert_eq!(metadata.accesses.len(), 5);
        assert!(metadata.name.ends_with("movement"));
    }

    #[test]
    fn conflicting_accesses_are_rejected_before_registration_or_execution() {
        let mut app = App::new();
        assert!(
            app.try_add_systems(FixedUpdate, |_: Query<(&mut Position, &Position)>| panic!(
                "invalid query ran"
            ))
            .is_err()
        );
        assert!(
            app.try_add_systems(
                FixedUpdate,
                |_: Query<&mut Position>, _: Query<&Position>| panic!("invalid queries ran")
            )
            .is_err()
        );
        assert!(
            app.try_add_systems(FixedUpdate, |_: ResMut<Scale>, _: Res<Scale>| panic!(
                "invalid resources ran"
            ))
            .is_err()
        );
        assert!(
            app.try_add_systems(FixedUpdate, |_: ResMut<Scale>, _: ResMut<Scale>| panic!(
                "duplicate writes ran"
            ))
            .is_err()
        );
        assert!(
            app.try_add_systems(FixedUpdate, |_: Commands, _: Commands| panic!(
                "duplicate commands ran"
            ))
            .is_err()
        );
        assert_eq!(app.system_metadata(FixedUpdate).count(), 0);
    }

    #[test]
    fn read_read_overlap_is_legal_and_component_resource_namespaces_are_distinct() {
        let mut app = App::new();
        let entity = app.world_mut().spawn();
        app.world_mut().insert(entity, Position(4)).unwrap();
        app.world_mut().insert_resource(Position(8));
        app.world_mut().insert_resource(Total::default());
        app.add_systems(
            FixedUpdate,
            |mut left: Query<(&Position, &Position)>,
             mut right: Query<&Position>,
             resource: Res<Position>,
             mut total: ResMut<Total>| {
                left.for_each(|_, (a, b)| total.0 += a.0 + b.0);
                right.for_each(|_, position| total.0 += position.0 + resource.0);
            },
        );
        app.try_advance_fixed(1).unwrap();
        assert_eq!(app.world().resource::<Total>().unwrap().0, 20);
    }

    #[test]
    fn missing_resources_stop_the_schedule_before_callback_and_report_checked_error() {
        let mut app = App::new();
        app.add_systems(FixedUpdate, |_: Res<Scale>| {
            panic!("missing resource callback ran")
        });
        app.add_systems(FixedUpdate, || panic!("later system ran"));
        let failures = app.try_advance_fixed(3).unwrap_err();
        assert!(matches!(
            &failures[0],
            AppError::System {
                error: SystemError::MissingResource { .. },
                ..
            }
        ));
        assert_eq!(
            app.world().resource::<crate::FixedTime>().unwrap().tick(),
            1
        );
    }

    #[test]
    fn explicit_deferred_node_controls_visibility_and_keeps_order() {
        let mut app = App::new();
        app.world_mut().insert_resource(Total::default());
        app.add_systems(FixedUpdate, |mut commands: Commands| {
            commands.spawn_with(Position(7));
        });
        app.add_systems(FixedUpdate, |mut query: Query<&Position>| {
            assert_eq!(query.for_each(|_, _| {}), 0);
        });
        app.add_systems(FixedUpdate, ApplyDeferred);
        app.add_systems(
            FixedUpdate,
            |mut query: Query<&Position>, mut total: ResMut<Total>| {
                query.for_each(|_, position| total.0 += position.0);
            },
        );
        app.try_advance_fixed(1).unwrap();
        assert_eq!(app.world().resource::<Total>().unwrap().0, 7);
        assert_eq!(
            app.system_metadata(FixedUpdate).nth(2).unwrap().name,
            "ApplyDeferred"
        );
    }

    #[test]
    fn four_component_query_and_sorted_order_handle_dense_swap_removal() {
        let mut app = App::new();
        let entities: Vec<_> = (0..3).map(|_| app.world_mut().spawn()).collect();
        for (index, &entity) in entities.iter().enumerate() {
            app.world_mut()
                .insert(entity, Position(index as i32))
                .unwrap();
            app.world_mut().insert(entity, Velocity(1)).unwrap();
            app.world_mut().insert(entity, Extra(2)).unwrap();
            app.world_mut().insert(entity, Fourth(3)).unwrap();
        }
        app.world_mut().remove::<Position>(entities[0]);
        app.world_mut().insert(entities[0], Position(0)).unwrap();
        app.world_mut().insert_resource(Vec::<crate::Entity>::new());
        app.add_systems(
            Startup,
            |mut query: Query<(&mut Position, &mut Velocity, &Extra, &Fourth)>,
             mut order: ResMut<Vec<crate::Entity>>| {
                query.for_each_sorted(|entity, (position, velocity, extra, fourth)| {
                    order.push(entity);
                    position.0 += extra.0;
                    velocity.0 += fourth.0;
                });
            },
        );
        app.try_advance_fixed(0).unwrap();
        assert_eq!(
            app.world().resource::<Vec<crate::Entity>>().unwrap(),
            &entities
        );
        for (index, &entity) in entities.iter().enumerate() {
            assert_eq!(
                app.world().get::<Position>(entity).unwrap().0,
                index as i32 + 2
            );
            assert_eq!(app.world().get::<Velocity>(entity).unwrap().0, 4);
        }
    }
}
