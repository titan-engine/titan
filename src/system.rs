//! Typed, access-validated systems executed sequentially by application schedules.
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
type Runner = Box<dyn FnMut(&mut World) -> Result<(), SystemError> + Send>;
/// A registered sequential system or explicit deferred-command boundary.
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
        self.runner.as_mut().expect("boundary handled by schedule")(world)
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
            runner: Some(Box::new(move |world| {
                self(world);
                Ok(())
            })),
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
            runner: Some(Box::new(move |_| {
                self();
                Ok(())
            })),
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
                Ok(System { metadata, runner: Some(Box::new(move |world| {
                    let mut context = SystemContext::prepare(world, &accesses)?;
                    $(let $value = $param::fetch(&mut context);)+
                    self($($value),+);
                    Ok(())
                })) })
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
