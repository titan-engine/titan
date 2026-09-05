//! Executor behavior is observable through ECS values and bounded handshakes.
#![cfg(not(target_arch = "wasm32"))]
use std::{
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use titan::{
    App, AppError, ApplyDeferred, Commands, Component, Entity, ExecutorPolicy, FixedUpdate, Query,
    Res, ResMut, SystemError, World,
};

#[derive(Component)]
struct Position(i32);
#[derive(Component)]
struct Velocity(i32);
struct Missing;

fn parallel(threads: usize) -> ExecutorPolicy {
    ExecutorPolicy::Parallel {
        max_threads: NonZeroUsize::new(threads).unwrap(),
    }
}
fn app() -> App {
    let mut app = App::new();
    app.set_executor_policy(parallel(2));
    app
}

/// Unlike a barrier, a scheduler regression fails instead of hanging the suite.
#[derive(Clone, Default)]
struct Rendezvous(Arc<(Mutex<usize>, Condvar)>);
impl Rendezvous {
    fn meet(&self) {
        let (lock, changed) = &*self.0;
        let mut arrived = lock.lock().unwrap();
        *arrived += 1;
        changed.notify_all();
        let (arrived, _) = changed
            .wait_timeout_while(arrived, Duration::from_secs(10), |arrived| *arrived < 2)
            .unwrap();
        assert_eq!(*arrived, 2, "compatible callbacks did not overlap");
    }
}

#[test]
fn shared_reads_and_disjoint_component_writes_really_overlap() {
    let mut app = app();
    let entity = app.world_mut().spawn();
    app.world_mut().insert(entity, Position(1)).unwrap();
    app.world_mut().insert(entity, Velocity(2)).unwrap();
    app.world_mut().insert_resource(7_i32);
    let left = Rendezvous::default();
    let right = left.clone();
    app.add_systems(
        FixedUpdate,
        move |mut query: Query<&mut Position>, shared: Res<i32>| {
            left.meet();
            query.for_each(|_, value| value.0 += *shared);
        },
    );
    app.add_systems(
        FixedUpdate,
        move |mut query: Query<&mut Velocity>, shared: Res<i32>| {
            right.meet();
            query.for_each(|_, value| value.0 += *shared);
        },
    );
    app.try_advance_fixed(1).unwrap();
    assert_eq!(app.world().get::<Position>(entity).unwrap().0, 8);
    assert_eq!(app.world().get::<Velocity>(entity).unwrap().0, 9);
}

#[test]
fn component_shared_reads_and_distinct_resource_writes_overlap() {
    let mut app = app();
    let entity = app.world_mut().spawn();
    app.world_mut().insert(entity, Position(3)).unwrap();
    app.world_mut().insert_resource(0_i32);
    app.world_mut().insert_resource(0_i64);
    let left = Rendezvous::default();
    let right = left.clone();
    app.add_systems(
        FixedUpdate,
        move |mut query: Query<&Position>, mut total: ResMut<i32>| {
            left.meet();
            query.for_each(|_, value| *total += value.0);
        },
    );
    app.add_systems(
        FixedUpdate,
        move |mut query: Query<&Position>, mut total: ResMut<i64>| {
            right.meet();
            query.for_each(|_, value| *total += i64::from(value.0));
        },
    );
    app.try_advance_fixed(1).unwrap();
    assert_eq!(app.world().resource::<i32>(), Some(&3));
    assert_eq!(app.world().resource::<i64>(), Some(&3));
}

#[test]
fn same_type_component_and_resource_writers_have_distinct_namespaces() {
    let mut app = app();
    let entity = app.world_mut().spawn();
    app.world_mut().insert(entity, Position(1)).unwrap();
    app.world_mut().insert_resource(Position(2));
    let left = Rendezvous::default();
    let right = left.clone();
    app.add_systems(FixedUpdate, move |mut query: Query<&mut Position>| {
        left.meet();
        query.for_each(|_, value| value.0 += 10);
    });
    app.add_systems(FixedUpdate, move |mut value: ResMut<Position>| {
        right.meet();
        value.0 += 20;
    });
    app.try_advance_fixed(1).unwrap();
    assert_eq!(app.world().get::<Position>(entity).unwrap().0, 11);
    assert_eq!(app.world().resource::<Position>().unwrap().0, 22);
}

#[test]
fn read_write_and_write_write_conflicts_keep_registration_order() {
    let mut app = app();
    let entity = app.world_mut().spawn();
    app.world_mut().insert(entity, Position(0)).unwrap();
    app.world_mut().insert_resource(0_i32);
    app.add_systems(FixedUpdate, |mut query: Query<&mut Position>| {
        query.for_each(|_, position| position.0 = 1);
    });
    app.add_systems(FixedUpdate, |mut query: Query<&Position>| {
        query.for_each(|_, position| assert_eq!(position.0, 1));
    });
    app.add_systems(FixedUpdate, |mut query: Query<&mut Position>| {
        query.for_each(|_, position| position.0 = position.0 * 10 + 2);
    });
    app.add_systems(FixedUpdate, |mut query: Query<&mut Position>| {
        query.for_each(|_, position| position.0 = position.0 * 10 + 3);
    });
    app.add_systems(FixedUpdate, |mut value: ResMut<i32>| *value = 1);
    app.add_systems(FixedUpdate, |value: Res<i32>| assert_eq!(*value, 1));
    app.add_systems(FixedUpdate, |mut value: ResMut<i32>| {
        *value = *value * 10 + 2
    });
    app.add_systems(FixedUpdate, |mut value: ResMut<i32>| {
        *value = *value * 10 + 3
    });
    app.try_advance_fixed(1).unwrap();
    assert_eq!(app.world().get::<Position>(entity).unwrap().0, 123);
    assert_eq!(app.world().resource::<i32>(), Some(&123));
}

fn command_result(policy: ExecutorPolicy) -> (Vec<Entity>, Vec<(Entity, i32)>) {
    let mut app = App::new();
    app.set_executor_policy(policy);
    app.world_mut().insert_resource(Vec::<Entity>::new());
    app.world_mut().insert_resource(Vec::<(Entity, i32)>::new());
    for value in [1, 2] {
        app.add_systems(
            FixedUpdate,
            move |mut commands: Commands, mut ids: ResMut<Vec<Entity>>| {
                let entity = commands.spawn_with(Position(value));
                commands.insert(entity, Position(value + 10));
                ids.push(entity);
            },
        );
    }
    app.add_systems(FixedUpdate, |mut query: Query<&Position>| {
        assert_eq!(
            query.for_each(|_, _| {}),
            0,
            "Commands barrier unexpectedly flushed"
        );
    });
    app.add_systems(FixedUpdate, |world: &mut World| {
        let ids = world.resource::<Vec<Entity>>().unwrap();
        assert!(
            ids.iter().all(|id| world.get::<Position>(*id).is_none()),
            "exclusive barrier unexpectedly flushed"
        );
    });
    app.add_systems(FixedUpdate, ApplyDeferred);
    app.add_systems(
        FixedUpdate,
        |mut query: Query<&Position>, mut values: ResMut<Vec<(Entity, i32)>>| {
            query.for_each_sorted(|id, position| values.push((id, position.0)));
        },
    );
    app.add_systems(FixedUpdate, |mut commands: Commands| {
        commands.spawn_with(Velocity(9));
    });
    app.try_advance_fixed(1).unwrap();
    assert_eq!(
        app.world()
            .iter::<Velocity>()
            .map(|(_, v)| v.0)
            .collect::<Vec<_>>(),
        [9]
    );
    (
        app.world().resource::<Vec<Entity>>().unwrap().clone(),
        app.world()
            .resource::<Vec<(Entity, i32)>>()
            .unwrap()
            .clone(),
    )
}

#[test]
fn commands_preserve_ids_order_and_only_explicit_or_final_flushes() {
    let sequential = command_result(ExecutorPolicy::Sequential);
    assert_eq!(
        sequential
            .1
            .iter()
            .map(|(_, value)| *value)
            .collect::<Vec<_>>(),
        [11, 12]
    );
    assert_eq!(command_result(parallel(2)), sequential);
}

#[test]
fn missing_resource_runs_valid_prefix_and_flushes_queued_commands() {
    let mut app = app();
    app.world_mut().insert_resource(0_i32);
    app.add_systems(FixedUpdate, |mut commands: Commands| {
        commands.spawn_with(Position(5));
    });
    app.add_systems(FixedUpdate, |mut value: ResMut<i32>| *value = 17);
    app.add_systems(FixedUpdate, |_: Res<Missing>| {
        panic!("missing-resource callback ran")
    });
    app.add_systems(FixedUpdate, || {
        panic!("callback after missing resource ran")
    });
    let errors = app.try_advance_fixed(1).unwrap_err();
    assert!(matches!(
        &errors[0],
        AppError::System {
            error: SystemError::MissingResource { .. },
            ..
        }
    ));
    assert_eq!(app.world().resource::<i32>(), Some(&17));
    assert_eq!(
        app.world()
            .iter::<Position>()
            .map(|(_, p)| p.0)
            .collect::<Vec<_>>(),
        [5]
    );
}

#[test]
fn exclusive_callback_observes_completed_preceding_batch() {
    let mut app = app();
    app.world_mut().insert_resource(0_i32);
    app.world_mut().insert_resource(0_i64);
    let left = Rendezvous::default();
    let right = left.clone();
    app.add_systems(FixedUpdate, move |mut value: ResMut<i32>| {
        left.meet();
        *value = 1;
    });
    app.add_systems(FixedUpdate, move |mut value: ResMut<i64>| {
        right.meet();
        *value = 2;
    });
    app.add_systems(FixedUpdate, |world: &mut World| {
        assert_eq!(world.resource::<i32>(), Some(&1));
        assert_eq!(world.resource::<i64>(), Some(&2));
        world.insert_resource(3_usize);
    });
    app.add_systems(FixedUpdate, |value: Res<usize>| assert_eq!(*value, 3));
    app.try_advance_fixed(1).unwrap();
}

#[test]
fn panic_joins_started_callbacks_and_does_not_start_later_batches() {
    let mut app = app();
    let rendezvous = Rendezvous::default();
    let other = rendezvous.clone();
    let joined = Arc::new(AtomicBool::new(false));
    let completed = joined.clone();
    let (sender, receiver) = mpsc::channel();
    struct SignalOnUnwind(mpsc::Sender<()>);
    impl Drop for SignalOnUnwind {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    app.add_systems(FixedUpdate, move || {
        let _signal = SignalOnUnwind(sender.clone());
        rendezvous.meet();
        panic!("deliberate callback panic");
    });
    app.add_systems(FixedUpdate, move || {
        other.meet();
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        completed.store(true, Ordering::SeqCst);
    });
    let later = Arc::new(AtomicBool::new(false));
    let later_callback = later.clone();
    app.add_systems(FixedUpdate, move |_: &mut World| {
        later_callback.store(true, Ordering::SeqCst);
    });
    assert!(catch_unwind(AssertUnwindSafe(|| app.try_advance_fixed(1))).is_err());
    assert!(!later.load(Ordering::SeqCst));
    assert!(
        joined.load(Ordering::SeqCst),
        "panic returned before sibling completed"
    );
}

#[test]
fn default_and_one_thread_run_callbacks_on_calling_thread_in_order() {
    for policy in [ExecutorPolicy::Sequential, parallel(1)] {
        let mut app = App::new();
        assert_eq!(app.executor_policy(), ExecutorPolicy::Sequential);
        app.set_executor_policy(policy);
        assert_eq!(app.executor_policy(), policy);
        let caller = std::thread::current().id();
        let order = Arc::new(Mutex::new(Vec::new()));
        for index in 0..4 {
            let order = order.clone();
            app.add_systems(FixedUpdate, move || {
                assert_eq!(std::thread::current().id(), caller);
                order.lock().unwrap().push(index);
            });
        }
        app.try_advance_fixed(1).unwrap();
        assert_eq!(*order.lock().unwrap(), [0, 1, 2, 3]);
    }
}

#[test]
fn deferred_failures_keep_order_and_continue_applying_the_queue() {
    fn run(policy: ExecutorPolicy) -> (Vec<AppError>, Vec<i32>, bool) {
        let mut app = App::new();
        app.set_executor_policy(policy);
        let entity = app.world_mut().spawn();
        app.add_systems(FixedUpdate, move |mut commands: Commands| {
            commands.despawn(entity).despawn(entity);
            commands.insert(entity, Position(99));
            commands.spawn_with(Position(7));
        });
        app.add_systems(FixedUpdate, ApplyDeferred);
        let ran = Arc::new(AtomicBool::new(false));
        let later = ran.clone();
        app.add_systems(FixedUpdate, move || {
            later.store(true, Ordering::SeqCst);
        });
        let errors = app.try_advance_fixed(1).unwrap_err();
        let values = app.world().iter::<Position>().map(|(_, p)| p.0).collect();
        (errors, values, ran.load(Ordering::SeqCst))
    }
    let sequential = run(ExecutorPolicy::Sequential);
    assert_eq!(sequential.0.len(), 2);
    assert_eq!(sequential.1, [7]);
    assert_eq!(run(parallel(2)), sequential);
}

#[test]
fn thread_limit_splits_compatible_callbacks_into_bounded_batches() {
    use std::sync::atomic::AtomicUsize;
    let mut app = app();
    let finished = Arc::new(AtomicUsize::new(0));
    for batch in 0..3 {
        let rendezvous = Rendezvous::default();
        for _ in 0..2 {
            let rendezvous = rendezvous.clone();
            let finished = finished.clone();
            app.add_systems(FixedUpdate, move || {
                assert!(
                    finished.load(Ordering::SeqCst) >= batch * 2,
                    "callback started before preceding bounded batch completed"
                );
                rendezvous.meet();
                finished.fetch_add(1, Ordering::SeqCst);
            });
        }
    }
    app.try_advance_fixed(1).unwrap();
    assert_eq!(finished.load(Ordering::SeqCst), 6);
}
