use std::{
    any::type_name,
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use titan::{
    App, ApplyDeferred, Commands, Component, DeferredOperation, Entity, ExecutorPolicy,
    FixedUpdate, Query, Res, ResMut, World,
};

#[derive(Component, Debug, PartialEq, Eq)]
struct Position(i32);

#[derive(Component, Debug, PartialEq, Eq)]
struct Velocity(i32);

#[derive(Component, Debug, PartialEq, Eq)]
struct Tag(u32);

#[derive(Component)]
struct DropToken {
    serial: u32,
    drops: Arc<AtomicUsize>,
}

impl Drop for DropToken {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ReferenceEntity {
    position: Option<i32>,
    velocity: Option<i32>,
    tag: Option<u32>,
}

#[derive(Clone, Debug)]
enum ReferenceCommand {
    Activate(Entity),
    InsertPosition(Entity, i32),
    InsertVelocity(Entity, i32),
    InsertBundle(Entity, i32, i32),
    Despawn(Entity),
}

#[derive(Default)]
struct ReferenceWorld {
    live: BTreeMap<Entity, ReferenceEntity>,
    reserved: BTreeSet<Entity>,
    deferred: Vec<ReferenceCommand>,
}

impl ReferenceWorld {
    fn spawn(&mut self, entity: Entity) {
        assert!(self.reserved.remove(&entity) || !self.live.contains_key(&entity));
        assert!(
            self.live
                .insert(entity, ReferenceEntity::default())
                .is_none()
        );
    }

    fn reserve(&mut self, entity: Entity) {
        assert!(!self.live.contains_key(&entity));
        assert!(self.reserved.insert(entity));
        self.deferred.push(ReferenceCommand::Activate(entity));
    }

    fn insert_position(&mut self, entity: Entity, value: i32) -> bool {
        let Some(state) = self.live.get_mut(&entity) else {
            return false;
        };
        state.position = Some(value);
        true
    }

    fn insert_velocity(&mut self, entity: Entity, value: i32) -> bool {
        let Some(state) = self.live.get_mut(&entity) else {
            return false;
        };
        state.velocity = Some(value);
        true
    }

    fn insert_bundle(&mut self, entity: Entity, position: i32, velocity: i32) -> bool {
        let Some(state) = self.live.get_mut(&entity) else {
            return false;
        };
        state.position = Some(position);
        state.velocity = Some(velocity);
        true
    }

    fn despawn(&mut self, entity: Entity) -> bool {
        self.live.remove(&entity).is_some()
    }

    fn apply_deferred(&mut self) -> Vec<(Entity, DeferredOperation)> {
        let commands = std::mem::take(&mut self.deferred);
        let mut errors = Vec::new();
        for command in commands {
            match command {
                ReferenceCommand::Activate(entity) => {
                    if self.reserved.remove(&entity) {
                        self.spawn(entity);
                    } else {
                        errors.push((entity, DeferredOperation::Spawn));
                    }
                }
                ReferenceCommand::InsertPosition(entity, value) => {
                    if !self.insert_position(entity, value) {
                        errors.push((entity, DeferredOperation::Insert));
                    }
                }
                ReferenceCommand::InsertVelocity(entity, value) => {
                    if !self.insert_velocity(entity, value) {
                        errors.push((entity, DeferredOperation::Insert));
                    }
                }
                ReferenceCommand::InsertBundle(entity, position, velocity) => {
                    if !self.insert_bundle(entity, position, velocity) {
                        errors.push((entity, DeferredOperation::Insert));
                    }
                }
                ReferenceCommand::Despawn(entity) => {
                    if !self.despawn(entity) {
                        errors.push((entity, DeferredOperation::Despawn));
                    }
                }
            }
        }
        errors
    }
}

#[derive(Clone, Copy)]
struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        // A small fixed LCG keeps failures reproducible without adding a dependency.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn usize(&mut self, upper: usize) -> usize {
        (self.next() as usize) % upper
    }

    fn i32(&mut self) -> i32 {
        (self.next() % 2_001) as i32 - 1_000
    }
}

fn choose_handle(rng: &mut DeterministicRng, handles: &[Entity]) -> Entity {
    handles[rng.usize(handles.len())]
}

fn context(seed: u64, trace: &[String]) -> String {
    format!("seed: {seed:#018x}\noperation trace:\n{}", trace.join("\n"))
}

fn assert_matches(world: &World, reference: &ReferenceWorld, seed: u64, trace: &[String]) {
    let context = context(seed, trace);
    let actual_entities = world.entities().collect::<Vec<_>>();
    let expected_entities = reference.live.keys().copied().collect::<Vec<_>>();
    assert_eq!(world.entity_count(), reference.live.len(), "{context}");
    assert_eq!(actual_entities, expected_entities, "{context}");

    for (entity, expected) in &reference.live {
        let actual_position = world.get::<Position>(*entity).map(|value| value.0);
        let actual_velocity = world.get::<Velocity>(*entity).map(|value| value.0);
        let actual_tag = world.get::<Tag>(*entity).map(|value| value.0);
        assert_eq!(actual_position, expected.position, "{context}");
        assert_eq!(actual_velocity, expected.velocity, "{context}");
        assert_eq!(actual_tag, expected.tag, "{context}");

        let mut expected_types = Vec::new();
        if expected.position.is_some() {
            expected_types.push(type_name::<Position>());
        }
        if expected.velocity.is_some() {
            expected_types.push(type_name::<Velocity>());
        }
        if expected.tag.is_some() {
            expected_types.push(type_name::<Tag>());
        }
        expected_types.sort_unstable();
        assert_eq!(
            world.component_type_names(*entity),
            expected_types,
            "{context}"
        );
    }

    let mut actual_positions = world
        .iter::<Position>()
        .map(|(entity, value)| (entity, value.0))
        .collect::<Vec<_>>();
    actual_positions.sort_unstable();
    let expected_positions = reference
        .live
        .iter()
        .filter_map(|(entity, state)| state.position.map(|value| (*entity, value)))
        .collect::<Vec<_>>();
    assert_eq!(actual_positions, expected_positions, "{context}");

    let mut actual_join = world
        .iter2::<Position, Velocity>()
        .map(|(entity, position, velocity)| (entity, position.0, velocity.0))
        .collect::<Vec<_>>();
    actual_join.sort_unstable();
    let expected_join = reference
        .live
        .iter()
        .filter_map(|(entity, state)| Some((*entity, state.position?, state.velocity?)))
        .collect::<Vec<_>>();
    assert_eq!(actual_join, expected_join, "{context}");
}

fn flush(world: &mut World, reference: &mut ReferenceWorld, seed: u64, trace: &mut Vec<String>) {
    trace.push("apply deferred".into());
    let actual = world
        .apply_deferred()
        .into_iter()
        .map(|error| (error.entity(), error.operation()))
        .collect::<Vec<_>>();
    let expected = reference.apply_deferred();
    assert_eq!(actual, expected, "{}", context(seed, trace));
    assert_matches(world, reference, seed, trace);
}

fn run_seed(seed: u64) {
    let mut rng = DeterministicRng::new(seed);
    let mut world = World::new();
    let mut reference = ReferenceWorld::default();
    let mut handles = Vec::new();
    let mut trace = Vec::new();

    for step in 0..96 {
        let operation = if handles.is_empty() { 0 } else { rng.usize(11) };
        match operation {
            0 => {
                let position = rng.i32();
                let velocity = rng.i32();
                let entity = world.spawn_with((Position(position), Velocity(velocity)));
                reference.spawn(entity);
                reference.insert_bundle(entity, position, velocity);
                handles.push(entity);
                trace.push(format!(
                    "{step}: spawn bundle -> {entity:?} ({position}, {velocity})"
                ));
            }
            1 => {
                let entity = choose_handle(&mut rng, &handles);
                let actual = world.despawn(entity);
                let expected = reference.despawn(entity);
                trace.push(format!("{step}: despawn {entity:?} -> {actual}"));
                assert_eq!(actual, expected, "{}", context(seed, &trace));
            }
            2 => {
                let entity = choose_handle(&mut rng, &handles);
                let value = rng.i32();
                let actual = world.insert(entity, Position(value)).is_ok();
                let expected = reference.insert_position(entity, value);
                trace.push(format!(
                    "{step}: insert position {entity:?} = {value} -> {actual}"
                ));
                assert_eq!(actual, expected, "{}", context(seed, &trace));
            }
            3 => {
                let entity = choose_handle(&mut rng, &handles);
                let actual = world.remove::<Velocity>(entity).map(|value| value.0);
                let expected = reference
                    .live
                    .get_mut(&entity)
                    .and_then(|state| state.velocity.take());
                trace.push(format!("{step}: remove velocity {entity:?} -> {actual:?}"));
                assert_eq!(actual, expected, "{}", context(seed, &trace));
            }
            4 => {
                let entity = choose_handle(&mut rng, &handles);
                let position = rng.i32();
                let velocity = rng.i32();
                let actual = world
                    .insert_bundle(entity, (Position(position), Velocity(velocity)))
                    .is_ok();
                let expected = reference.insert_bundle(entity, position, velocity);
                trace.push(format!(
                    "{step}: insert bundle {entity:?} = ({position}, {velocity}) -> {actual}"
                ));
                assert_eq!(actual, expected, "{}", context(seed, &trace));
            }
            5 => {
                let value = rng.i32();
                let entity = world.commands().spawn_with(Position(value));
                reference.reserve(entity);
                reference
                    .deferred
                    .push(ReferenceCommand::InsertPosition(entity, value));
                handles.push(entity);
                trace.push(format!(
                    "{step}: defer spawn {entity:?} with position {value}"
                ));
                assert!(!world.is_alive(entity), "{}", context(seed, &trace));
            }
            6 => {
                let entity = choose_handle(&mut rng, &handles);
                let value = rng.i32();
                world.commands().insert(entity, Position(value));
                reference
                    .deferred
                    .push(ReferenceCommand::InsertPosition(entity, value));
                trace.push(format!("{step}: defer position {entity:?} = {value}"));
            }
            7 => {
                let entity = choose_handle(&mut rng, &handles);
                let value = rng.i32();
                world.commands().insert(entity, Velocity(value));
                reference
                    .deferred
                    .push(ReferenceCommand::InsertVelocity(entity, value));
                trace.push(format!("{step}: defer velocity {entity:?} = {value}"));
            }
            8 => {
                let entity = choose_handle(&mut rng, &handles);
                let position = rng.i32();
                let velocity = rng.i32();
                world
                    .commands()
                    .insert_bundle(entity, (Position(position), Velocity(velocity)));
                reference
                    .deferred
                    .push(ReferenceCommand::InsertBundle(entity, position, velocity));
                trace.push(format!(
                    "{step}: defer bundle {entity:?} = ({position}, {velocity})"
                ));
            }
            9 => {
                let entity = choose_handle(&mut rng, &handles);
                world.commands().despawn(entity);
                reference.deferred.push(ReferenceCommand::Despawn(entity));
                trace.push(format!("{step}: defer despawn {entity:?}"));
            }
            10 => flush(&mut world, &mut reference, seed, &mut trace),
            _ => unreachable!(),
        }
        assert_matches(&world, &reference, seed, &trace);
    }
    flush(&mut world, &mut reference, seed, &mut trace);
}

#[test]
fn seeded_structural_sequences_match_independent_reference_world() {
    for seed in [
        0,
        1,
        0x61,
        0x5eed,
        0xdead_beef,
        0x1234_5678_9abc_def0,
        u64::MAX - 1,
        u64::MAX,
    ] {
        run_seed(seed);
    }
}

#[test]
fn stale_deferred_commands_never_reach_a_recycled_entity() {
    let mut world = World::new();
    let stale = world.spawn_with(Position(1));
    assert!(world.despawn(stale));
    let replacement = world.spawn_with(Position(2));
    assert_eq!(replacement.index(), stale.index());
    assert_ne!(replacement.generation(), stale.generation());

    world
        .commands()
        .insert(stale, Position(999))
        .despawn(stale)
        .insert(replacement, Position(7));
    let errors = world.apply_deferred();

    assert_eq!(
        errors
            .iter()
            .map(|error| (error.entity(), error.operation()))
            .collect::<Vec<_>>(),
        [
            (stale, DeferredOperation::Insert),
            (stale, DeferredOperation::Despawn),
        ]
    );
    assert!(world.is_alive(replacement));
    assert_eq!(world.get::<Position>(replacement), Some(&Position(7)));
}

#[test]
fn components_drop_once_on_replacement_removal_despawn_and_world_teardown() {
    let drops = Arc::new(AtomicUsize::new(0));
    {
        let mut world = World::new();
        let entity = world.spawn();

        assert!(
            world
                .insert(
                    entity,
                    DropToken {
                        serial: 1,
                        drops: drops.clone(),
                    },
                )
                .unwrap()
                .is_none()
        );
        let replaced = world
            .insert(
                entity,
                DropToken {
                    serial: 2,
                    drops: drops.clone(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(replaced.serial, 1);
        drop(replaced);
        assert_eq!(drops.load(Ordering::SeqCst), 1);

        let removed = world.remove::<DropToken>(entity).unwrap();
        assert_eq!(removed.serial, 2);
        drop(removed);
        assert_eq!(drops.load(Ordering::SeqCst), 2);

        world
            .insert(
                entity,
                DropToken {
                    serial: 3,
                    drops: drops.clone(),
                },
            )
            .unwrap();
        assert!(world.despawn(entity));
        assert_eq!(drops.load(Ordering::SeqCst), 3);

        let teardown = world.spawn_with(DropToken {
            serial: 4,
            drops: drops.clone(),
        });
        assert_eq!(world.get::<DropToken>(teardown).unwrap().serial, 4);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 4);
}

#[derive(Clone, Copy)]
struct Tick(u32);

#[derive(Default)]
struct ScriptState {
    initial: Vec<Entity>,
    spawned: Vec<Entity>,
    stale: Option<Entity>,
}

#[derive(Debug, PartialEq, Eq)]
struct AppResult {
    errors: Vec<(Entity, DeferredOperation)>,
    entities: Vec<Entity>,
    positions: Vec<(Entity, i32)>,
    velocities: Vec<(Entity, i32)>,
    joined: Vec<(Entity, i32, i32)>,
}

fn run_executor_script(policy: ExecutorPolicy) -> AppResult {
    let mut app = App::new();
    app.set_executor_policy(policy);
    let mut initial = Vec::new();
    for value in 0..6 {
        initial.push(app.world_mut().spawn_with((
            Position(value),
            Velocity(value * 10),
            Tag(value as u32),
        )));
    }
    app.world_mut().insert_resource(Tick(0));
    app.world_mut().insert_resource(ScriptState {
        initial,
        ..ScriptState::default()
    });

    app.add_systems(
        FixedUpdate,
        |mut positions: Query<&mut Position>, tick: Res<Tick>| {
            positions.for_each(|_, position| position.0 += tick.0 as i32 + 1);
        },
    );
    app.add_systems(
        FixedUpdate,
        |mut velocities: Query<&mut Velocity>, tick: Res<Tick>| {
            velocities.for_each(|_, velocity| velocity.0 -= tick.0 as i32 + 1);
        },
    );
    app.add_systems(
        FixedUpdate,
        |mut commands: Commands, tick: Res<Tick>, mut state: ResMut<ScriptState>| match tick.0 {
            0 => {
                let entity = commands.spawn_with((Position(20), Velocity(200), Tag(20)));
                state.spawned.push(entity);
            }
            1 => {
                let stale = state.initial[1];
                commands.despawn(stale);
                state.stale = Some(stale);
            }
            2 => {
                let entity = commands.spawn_with((Position(30), Velocity(300)));
                state.spawned.push(entity);
            }
            3 => {
                commands
                    .insert(state.stale.unwrap(), Position(999))
                    .insert(state.spawned[1], Position(33));
            }
            4 => {
                commands.insert_bundle(state.initial[3], (Position(44), Velocity(444)));
            }
            5 => {
                commands.despawn(state.spawned[0]);
            }
            _ => unreachable!(),
        },
    );
    app.add_systems(FixedUpdate, |mut tick: ResMut<Tick>| tick.0 += 1);
    app.add_systems(FixedUpdate, ApplyDeferred);

    let mut errors = Vec::new();
    for _ in 0..6 {
        if let Err(step_errors) = app.try_advance_fixed(1) {
            errors.extend(step_errors.into_iter().filter_map(|error| match error {
                titan::AppError::Deferred(error) => Some((error.entity(), error.operation())),
                titan::AppError::System { .. } => None,
            }));
        }
    }

    let world = app.world();
    let entities = world.entities().collect();
    let mut positions = world
        .iter::<Position>()
        .map(|(entity, value)| (entity, value.0))
        .collect::<Vec<_>>();
    positions.sort_unstable();
    let mut velocities = world
        .iter::<Velocity>()
        .map(|(entity, value)| (entity, value.0))
        .collect::<Vec<_>>();
    velocities.sort_unstable();
    let mut joined = world
        .iter2::<Position, Velocity>()
        .map(|(entity, position, velocity)| (entity, position.0, velocity.0))
        .collect::<Vec<_>>();
    joined.sort_unstable();
    AppResult {
        errors,
        entities,
        positions,
        velocities,
        joined,
    }
}

#[test]
fn sequential_and_parallel_executors_produce_the_same_structural_result() {
    let sequential = run_executor_script(ExecutorPolicy::Sequential);
    let parallel = run_executor_script(ExecutorPolicy::Parallel {
        max_threads: NonZeroUsize::new(2).unwrap(),
    });
    assert_eq!(parallel, sequential);
    assert_eq!(sequential.errors.len(), 1);
    assert_eq!(sequential.errors[0].1, DeferredOperation::Insert);
}
