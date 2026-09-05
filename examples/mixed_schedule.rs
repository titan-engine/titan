//! Deterministic mixed-schedule executor measurement. See docs/executor-overhead.md.

use std::{env, hint::black_box, num::NonZeroUsize, process::ExitCode, time::Instant};
use titan::{
    AccessMode, AccessTarget, App, ApplyDeferred, Commands, Component, ExecutorPolicy, FixedTime,
    FixedUpdate, Query, ResMut, SystemAccess, SystemMetadata,
};

const REPEATS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Config {
    entities: u32,
    steps: u64,
    work_iterations: u32,
    threads: NonZeroUsize,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Config>, String> {
    let mut config = Config {
        entities: 1_000,
        steps: 120,
        work_iterations: 16,
        threads: NonZeroUsize::MIN,
    };
    let mut args = args.into_iter();
    let mut seen = [false; 4];
    while let Some(flag) = args.next() {
        if matches!(flag.as_str(), "--help" | "-h") {
            return Ok(None);
        }
        let slot = match flag.as_str() {
            "--entities" => 0,
            "--steps" => 1,
            "--work-iterations" => 2,
            "--threads" => 3,
            _ => return Err(format!("unknown argument: {flag}")),
        };
        if seen[slot] {
            return Err(format!("duplicate argument: {flag}"));
        }
        seen[slot] = true;
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--entities" => {
                config.entities = value
                    .parse()
                    .map_err(|_| "--entities must be an integer in 0..4294967294")?;
                if config.entities == u32::MAX {
                    return Err("--entities must be smaller than 4294967295".into());
                }
            }
            "--steps" => {
                config.steps = value
                    .parse()
                    .map_err(|_| "--steps must be a nonnegative u64 integer")?
            }
            "--work-iterations" => {
                config.work_iterations = value
                    .parse()
                    .map_err(|_| "--work-iterations must be an integer in 1..=536870911")?;
                if !(1..=u32::MAX / 8).contains(&config.work_iterations) {
                    return Err("--work-iterations must be an integer in 1..=536870911".into());
                }
            }
            "--threads" => {
                config.threads = value
                    .parse()
                    .map_err(|_| "--threads must be a positive integer")?
            }
            _ => unreachable!(),
        }
    }
    Ok(Some(config))
}

#[derive(Component)]
struct SmallA(u64);
#[derive(Component)]
struct SmallB(u64);
#[derive(Component)]
struct SmallC(u64);
#[derive(Component)]
struct SmallD(u64);

fn small_a(mut q: Query<&mut SmallA>) {
    q.for_each(|_, v| v.0 = v.0.wrapping_add(1));
}
fn small_b(mut q: Query<&mut SmallB>) {
    q.for_each(|_, v| v.0 = v.0.wrapping_add(3));
}
fn small_c(mut q: Query<&mut SmallC>) {
    q.for_each(|_, v| v.0 = v.0.wrapping_add(5));
}
fn small_d(mut q: Query<&mut SmallD>) {
    q.for_each(|_, v| v.0 = v.0.wrapping_add(7));
}

#[derive(Component)]
struct UnevenA(u64);
#[derive(Component)]
struct UnevenB(u64);
#[derive(Component)]
struct UnevenC(u64);
#[derive(Component)]
struct UnevenD(u64);

fn mix(mut value: u64, rounds: u32) -> u64 {
    for round in 0..rounds {
        value = black_box(value)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(u64::from(round) + 1442695040888963407);
    }
    value
}

// Keep the oracle independent from the measured callback helper. This spells
// the recurrence differently and deliberately does not call `mix` or black_box.
fn reference_mix(initial: u64, rounds: u32) -> u64 {
    (0..rounds).fold(initial, |value, round| {
        let multiplied = value.wrapping_mul(6_364_136_223_846_793_005);
        multiplied
            .wrapping_add(1_442_695_040_888_963_407)
            .wrapping_add(u64::from(round))
    })
}
fn uneven_a(mut q: Query<&mut UnevenA>, work: titan::Res<Work>) {
    q.for_each(|_, v| v.0 = mix(v.0, work.0));
}
fn uneven_b(mut q: Query<&mut UnevenB>, work: titan::Res<Work>) {
    q.for_each(|_, v| v.0 = mix(v.0, work.0 * 8));
}
fn uneven_c(mut q: Query<&mut UnevenC>, work: titan::Res<Work>) {
    q.for_each(|_, v| v.0 = mix(v.0, work.0));
}
fn uneven_d(mut q: Query<&mut UnevenD>, work: titan::Res<Work>) {
    q.for_each(|_, v| v.0 = mix(v.0, work.0 * 4));
}
struct Work(u32);

#[derive(Component)]
struct ConflictA(u64);
#[derive(Component)]
struct ConflictB(u64);
#[derive(Default)]
struct Observed(u128);
fn conflict_first(mut q: Query<&mut ConflictA>) {
    q.for_each(|_, v| v.0 += 1);
}
fn conflict_observe(mut q: Query<&ConflictA>, mut observed: ResMut<Observed>) {
    q.for_each(|_, v| observed.0 += u128::from(v.0));
}
fn conflict_independent(mut q: Query<&mut ConflictB>) {
    q.for_each(|_, v| v.0 += 11);
}
fn conflict_last(mut q: Query<&mut ConflictA>) {
    q.for_each(|_, v| v.0 += 2);
}

#[derive(Component)]
struct CommandBase(u64);
#[derive(Component)]
struct Spawned(u64);
#[derive(Component)]
struct CommandSide(u64);
fn command_before(mut q: Query<&mut CommandBase>) {
    q.for_each(|_, v| v.0 += 2);
}
fn command_spawn(mut commands: Commands) {
    commands.spawn_with(Spawned(10));
}
fn command_after(mut q: Query<&mut Spawned>) {
    q.for_each(|_, v| v.0 += 1);
}
fn command_side(mut q: Query<&mut CommandSide>) {
    q.for_each(|_, v| v.0 += 5);
}

#[derive(serde::Serialize)]
struct Timings {
    initialization_ns: u64,
    simulation_ns: u64,
    validation_ns: u64,
}

#[derive(serde::Serialize)]
struct BatchEvidence {
    system_kinds: Vec<&'static str>,
    batch_sizes: Vec<usize>,
    prepared_batches_per_tick: usize,
    prepared_contexts_per_tick: usize,
    worker_dispatches_per_tick: usize,
    sequential_callbacks_per_tick: usize,
    conflict_splits_per_tick: usize,
    commands_barriers_per_tick: usize,
    apply_deferred_barriers_per_tick: usize,
}

#[derive(serde::Serialize)]
struct ScenarioRun {
    name: &'static str,
    checksum: String,
    correctness: bool,
    timings: Timings,
    schedule: BatchEvidence,
}

fn compatible(left: &[SystemAccess], right: &[SystemAccess]) -> bool {
    left.iter().all(|a| {
        right.iter().all(|b| {
            a.target != b.target
                || a.type_name != b.type_name
                || (a.mode == AccessMode::Read && b.mode == AccessMode::Read)
        })
    })
}

fn parallel_candidate(system: &SystemMetadata) -> bool {
    system.name != "ApplyDeferred"
        && system.accesses.iter().all(|access| {
            matches!(
                access.target,
                AccessTarget::Component | AccessTarget::Resource
            )
        })
}

fn batch_evidence(app: &App, threads: usize) -> BatchEvidence {
    let systems: Vec<_> = app.system_metadata(FixedUpdate).collect();
    let mut sizes = Vec::new();
    let mut index = 0;
    let mut conflict_splits = 0;
    while index < systems.len() {
        if threads > 1 {
            let mut end = index;
            while end < systems.len() && end - index < threads {
                let candidate = systems[end];
                if !parallel_candidate(candidate) {
                    break;
                }
                if systems[index..end]
                    .iter()
                    .any(|prior| !compatible(&prior.accesses, &candidate.accesses))
                {
                    conflict_splits += 1;
                    break;
                }
                end += 1;
            }
            if end - index > 1 {
                sizes.push(end - index);
                index = end;
                continue;
            }
        }
        sizes.push(1);
        index += 1;
    }
    let prepared_batches = sizes.iter().filter(|&&size| size > 1).count();
    let prepared_contexts = sizes.iter().filter(|&&size| size > 1).sum();
    let sequential_callbacks = sizes.iter().filter(|&&size| size == 1).count();
    let commands_barriers = systems
        .iter()
        .filter(|system| {
            system
                .accesses
                .iter()
                .any(|access| access.target == AccessTarget::Commands)
        })
        .count();
    let apply_deferred_barriers = systems
        .iter()
        .filter(|system| system.name == "ApplyDeferred")
        .count();
    BatchEvidence {
        system_kinds: systems
            .iter()
            .map(|system| {
                if system.name == "ApplyDeferred" {
                    "apply_deferred_barrier"
                } else if system
                    .accesses
                    .iter()
                    .any(|access| access.target == AccessTarget::Commands)
                {
                    "commands_barrier"
                } else if system
                    .accesses
                    .iter()
                    .any(|access| access.target == AccessTarget::World)
                {
                    "exclusive_barrier"
                } else {
                    "typed"
                }
            })
            .collect(),
        batch_sizes: sizes,
        prepared_batches_per_tick: prepared_batches,
        prepared_contexts_per_tick: prepared_contexts,
        worker_dispatches_per_tick: prepared_contexts,
        sequential_callbacks_per_tick: sequential_callbacks,
        conflict_splits_per_tick: conflict_splits,
        commands_barriers_per_tick: commands_barriers,
        apply_deferred_barriers_per_tick: apply_deferred_barriers,
    }
}

fn elapsed_ns(start: Instant) -> Result<u64, String> {
    start
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| "timing exceeds u64 nanoseconds".into())
}

fn hash(values: impl IntoIterator<Item = u64>) -> u64 {
    values
        .into_iter()
        .fold(0xcbf29ce484222325, |mut hash, value| {
            for byte in value.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash
        })
}

fn configure(app: &mut App, config: Config) {
    if config.threads.get() > 1 {
        app.set_executor_policy(ExecutorPolicy::Parallel {
            max_threads: config.threads,
        });
    }
}

fn finish(app: &mut App, config: Config, started: Instant) -> Result<(u64, Timings), String> {
    let initialization_ns = elapsed_ns(started)?;
    let started = Instant::now();
    app.try_advance_fixed(config.steps)
        .map_err(|errors| format!("simulation failed: {errors:?}"))?;
    let simulation_ns = elapsed_ns(started)?;
    let started = Instant::now();
    let tick = app
        .world()
        .resource::<FixedTime>()
        .ok_or("missing FixedTime")?
        .tick();
    if tick != config.steps {
        return Err("fixed tick mismatch".into());
    }
    Ok((
        tick,
        Timings {
            initialization_ns,
            simulation_ns,
            validation_ns: elapsed_ns(started)?,
        },
    ))
}

fn small(config: Config) -> Result<ScenarioRun, String> {
    let started = Instant::now();
    let mut app = App::new();
    configure(&mut app, config);
    for id in 0..config.entities {
        let n = u64::from(id);
        app.world_mut()
            .spawn_with((SmallA(n), SmallB(n * 2), SmallC(n * 3), SmallD(n * 4)));
    }
    app.add_systems(FixedUpdate, small_a)
        .add_systems(FixedUpdate, small_b)
        .add_systems(FixedUpdate, small_c)
        .add_systems(FixedUpdate, small_d);
    let schedule = batch_evidence(&app, config.threads.get());
    let (_, mut timings) = finish(&mut app, config, started)?;
    let started = Instant::now();
    let mut values = Vec::with_capacity(config.entities as usize * 4);
    for (index, entity) in app.world().entities().enumerate() {
        let n = index as u64;
        let steps = config.steps;
        let actual = [
            app.world().get::<SmallA>(entity).ok_or("missing SmallA")?.0,
            app.world().get::<SmallB>(entity).ok_or("missing SmallB")?.0,
            app.world().get::<SmallC>(entity).ok_or("missing SmallC")?.0,
            app.world().get::<SmallD>(entity).ok_or("missing SmallD")?.0,
        ];
        let expected = [
            n + steps,
            n * 2 + 3 * steps,
            n * 3 + 5 * steps,
            n * 4 + 7 * steps,
        ];
        if actual != expected {
            return Err(format!("small state mismatch at {index}"));
        }
        values.extend(actual);
    }
    timings.validation_ns = elapsed_ns(started)?;
    Ok(ScenarioRun {
        name: "small_compatible",
        checksum: format!("{:016x}", hash(values)),
        correctness: true,
        timings,
        schedule,
    })
}

fn uneven(config: Config) -> Result<ScenarioRun, String> {
    let started = Instant::now();
    let mut app = App::new();
    configure(&mut app, config);
    app.world_mut()
        .insert_resource(Work(config.work_iterations));
    for id in 0..config.entities {
        let n = u64::from(id) + 1;
        app.world_mut()
            .spawn_with((UnevenA(n), UnevenB(n), UnevenC(n), UnevenD(n)));
    }
    app.add_systems(FixedUpdate, uneven_a)
        .add_systems(FixedUpdate, uneven_b)
        .add_systems(FixedUpdate, uneven_c)
        .add_systems(FixedUpdate, uneven_d);
    let schedule = batch_evidence(&app, config.threads.get());
    let (_, mut timings) = finish(&mut app, config, started)?;
    let started = Instant::now();
    let mut values = Vec::with_capacity(config.entities as usize * 4);
    for (index, entity) in app.world().entities().enumerate() {
        let initial = index as u64 + 1;
        let expected = [
            config.work_iterations,
            config.work_iterations * 8,
            config.work_iterations,
            config.work_iterations * 4,
        ]
        .map(|rounds| (0..config.steps).fold(initial, |v, _| reference_mix(v, rounds)));
        let actual = [
            app.world()
                .get::<UnevenA>(entity)
                .ok_or("missing UnevenA")?
                .0,
            app.world()
                .get::<UnevenB>(entity)
                .ok_or("missing UnevenB")?
                .0,
            app.world()
                .get::<UnevenC>(entity)
                .ok_or("missing UnevenC")?
                .0,
            app.world()
                .get::<UnevenD>(entity)
                .ok_or("missing UnevenD")?
                .0,
        ];
        if actual != expected {
            return Err(format!("uneven state mismatch at {index}"));
        }
        values.extend(actual);
    }
    timings.validation_ns = elapsed_ns(started)?;
    Ok(ScenarioRun {
        name: "uneven_compatible",
        checksum: format!("{:016x}", hash(values)),
        correctness: true,
        timings,
        schedule,
    })
}

fn conflicts(config: Config) -> Result<ScenarioRun, String> {
    let started = Instant::now();
    let mut app = App::new();
    configure(&mut app, config);
    app.world_mut().insert_resource(Observed::default());
    for id in 0..config.entities {
        let n = u64::from(id);
        app.world_mut().spawn_with((ConflictA(n), ConflictB(n * 2)));
    }
    app.add_systems(FixedUpdate, conflict_first)
        .add_systems(FixedUpdate, conflict_observe)
        .add_systems(FixedUpdate, conflict_independent)
        .add_systems(FixedUpdate, conflict_last);
    let schedule = batch_evidence(&app, config.threads.get());
    let (_, mut timings) = finish(&mut app, config, started)?;
    let started = Instant::now();
    let mut values = Vec::with_capacity(config.entities as usize * 2 + 1);
    let mut expected_observed = 0_u128;
    for tick in 0..config.steps {
        for id in 0..config.entities {
            expected_observed += u128::from(id) + u128::from(tick) * 3 + 1;
        }
    }
    let actual_observed = app
        .world()
        .resource::<Observed>()
        .ok_or("missing Observed")?
        .0;
    if actual_observed != expected_observed {
        return Err("conflict observation mismatch".into());
    }
    for (index, entity) in app.world().entities().enumerate() {
        let n = index as u64;
        let actual = [
            app.world()
                .get::<ConflictA>(entity)
                .ok_or("missing ConflictA")?
                .0,
            app.world()
                .get::<ConflictB>(entity)
                .ok_or("missing ConflictB")?
                .0,
        ];
        let expected = [n + 3 * config.steps, n * 2 + 11 * config.steps];
        if actual != expected {
            return Err(format!("conflict state mismatch at {index}"));
        }
        values.extend(actual);
    }
    values.extend([actual_observed as u64, (actual_observed >> 64) as u64]);
    timings.validation_ns = elapsed_ns(started)?;
    // ConflictA forces the first and last callbacks to remain singleton; the middle pair is compatible.
    Ok(ScenarioRun {
        name: "conflicts",
        checksum: format!("{:016x}", hash(values)),
        correctness: true,
        timings,
        schedule,
    })
}

fn commands(config: Config) -> Result<ScenarioRun, String> {
    let started = Instant::now();
    let mut app = App::new();
    configure(&mut app, config);
    for id in 0..config.entities {
        app.world_mut()
            .spawn_with((CommandBase(u64::from(id)), CommandSide(u64::from(id) * 3)));
    }
    app.add_systems(FixedUpdate, command_before)
        .add_systems(FixedUpdate, command_spawn)
        .add_systems(FixedUpdate, ApplyDeferred)
        .add_systems(FixedUpdate, command_after)
        .add_systems(FixedUpdate, command_side);
    let schedule = batch_evidence(&app, config.threads.get());
    let (_, mut timings) = finish(&mut app, config, started)?;
    let started = Instant::now();
    let mut base_count = 0_u32;
    let mut spawned = Vec::new();
    let mut values = Vec::new();
    for entity in app.world().entities() {
        if let Some(value) = app.world().get::<CommandBase>(entity) {
            let expected = u64::from(base_count) + 2 * config.steps;
            if value.0 != expected {
                return Err("command base mismatch".into());
            }
            let side = app
                .world()
                .get::<CommandSide>(entity)
                .ok_or("missing CommandSide")?
                .0;
            if side != u64::from(base_count) * 3 + 5 * config.steps {
                return Err("command side mismatch".into());
            }
            values.extend([value.0, side]);
            base_count += 1;
        }
        if let Some(value) = app.world().get::<Spawned>(entity) {
            spawned.push(value.0);
            values.push(value.0);
        }
    }
    let expected_spawned: Vec<_> = (0..config.steps)
        .map(|tick| 10 + config.steps - tick)
        .collect();
    if base_count != config.entities
        || spawned != expected_spawned
        || app.world().entity_count() != config.entities as usize + config.steps as usize
    {
        return Err("commands barrier state mismatch".into());
    }
    timings.validation_ns = elapsed_ns(started)?;
    Ok(ScenarioRun {
        name: "commands_barriers",
        checksum: format!("{:016x}", hash(values)),
        correctness: true,
        timings,
        schedule,
    })
}

fn run_all(config: Config) -> Result<Vec<ScenarioRun>, String> {
    Ok(vec![
        small(config)?,
        uneven(config)?,
        conflicts(config)?,
        commands(config)?,
    ])
}

fn execute(config: Config) -> Result<serde_json::Value, String> {
    let mut expected = None;
    let mut runs = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let scenarios = run_all(config)?;
        let checksums: Vec<_> = scenarios
            .iter()
            .map(|scenario| scenario.checksum.clone())
            .collect();
        if expected.as_ref().is_some_and(|prior| prior != &checksums) {
            return Err("repeat checksum mismatch".into());
        }
        expected = Some(checksums);
        runs.push(scenarios);
    }
    Ok(serde_json::json!({
        "schema_version": 1, "workload": "mixed_schedule", "entities": config.entities, "steps": config.steps,
        "work_iterations": config.work_iterations, "executor": if config.threads.get() == 1 { "sequential" } else { "parallel" },
        "max_threads": config.threads.get(), "repeats": REPEATS,
        "evidence_scope": "batch counts reproduce the documented executor rules for this fixed fixture; timings are workload-level and do not instrument executor internals",
        "correctness": {"independent_expected_state": true, "repeat_agreement": true}, "runs": runs,
        "environment": {"os": env::consts::OS, "arch": env::consts::ARCH, "debug_assertions": cfg!(debug_assertions)}
    }))
}

fn main() -> ExitCode {
    let result = parse_args(env::args().skip(1)).and_then(|config| match config {
        Some(config) => execute(config).map(|report| println!("{report}")),
        None => { println!("Usage: mixed_schedule [--entities N] [--steps N] [--work-iterations N] [--threads N]\nDefaults: --entities 1000 --steps 120 --work-iterations 16 --threads 1."); Ok(()) }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mixed_schedule: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_policies_match_independent_expected_state() {
        for entities in [0, 1, 17] {
            for steps in [0, 1, 7] {
                let mut baseline = None;
                for threads in [1, 2, 4] {
                    let config = Config {
                        entities,
                        steps,
                        work_iterations: 2,
                        threads: NonZeroUsize::new(threads).unwrap(),
                    };
                    let checksums: Vec<_> = run_all(config)
                        .unwrap()
                        .into_iter()
                        .map(|run| run.checksum)
                        .collect();
                    if let Some(expected) = &baseline {
                        assert_eq!(&checksums, expected);
                    } else {
                        baseline = Some(checksums);
                    }
                }
            }
        }
    }

    #[test]
    fn batch_evidence_covers_limits_conflicts_and_barriers() {
        let runs = run_all(Config {
            entities: 1,
            steps: 1,
            work_iterations: 1,
            threads: NonZeroUsize::new(3).unwrap(),
        })
        .unwrap();
        assert_eq!(runs[0].schedule.batch_sizes, [3, 1]);
        assert_eq!(runs[1].schedule.worker_dispatches_per_tick, 3);
        assert_eq!(runs[2].schedule.batch_sizes, [1, 2, 1]);
        assert_eq!(runs[2].schedule.conflict_splits_per_tick, 2);
        assert_eq!(runs[3].schedule.batch_sizes, [1, 1, 1, 2]);
        assert_eq!(runs[3].schedule.commands_barriers_per_tick, 1);
        assert_eq!(runs[3].schedule.apply_deferred_barriers_per_tick, 1);

        for (threads, compatible_batches, conflict_batches, command_batches) in [
            (1, vec![1, 1, 1, 1], vec![1, 1, 1, 1], vec![1, 1, 1, 1, 1]),
            (2, vec![2, 2], vec![1, 2, 1], vec![1, 1, 1, 2]),
            (4, vec![4], vec![1, 2, 1], vec![1, 1, 1, 2]),
        ] {
            let runs = run_all(Config {
                entities: 0,
                steps: 0,
                work_iterations: 1,
                threads: NonZeroUsize::new(threads).unwrap(),
            })
            .unwrap();
            assert_eq!(runs[0].schedule.batch_sizes, compatible_batches);
            assert_eq!(runs[2].schedule.batch_sizes, conflict_batches);
            assert_eq!(runs[3].schedule.batch_sizes, command_batches);
        }
    }

    #[test]
    fn parser_rejects_invalid_configuration() {
        for args in [
            vec!["--entities"],
            vec!["--entities", "4294967295"],
            vec!["--steps", "-1"],
            vec!["--work-iterations", "0"],
            vec!["--work-iterations", "536870912"],
            vec!["--threads", "0"],
            vec!["--threads", "2", "--threads", "3"],
            vec!["--wat"],
        ] {
            assert!(parse_args(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn uneven_kernel_and_independent_oracle_match_external_vectors() {
        // Values were calculated independently with integer arithmetic modulo
        // 2^64, so coupled edits to the workload and oracle cannot self-confirm.
        for (initial, rounds, expected) in [
            (1, 1, 0x6c57_6fac_43fd_007c),
            (1, 4, 0x7b06_d6c9_ac4e_040b),
            (1, 8, 0x0de9_b312_8619_0ce5),
            (17, 16, 0xec17_83c3_566d_9519),
            (2, 32, 0xe211_15bc_bbfb_eb92),
        ] {
            assert_eq!(mix(initial, rounds), expected);
            assert_eq!(reference_mix(initial, rounds), expected);
        }
    }
}
