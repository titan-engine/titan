//! Deterministic headless patrol-and-fire workload. See docs/swarm.md.
use std::{env, mem::size_of, num::NonZeroUsize, process::ExitCode, time::Instant};
use titan::{App, Component, ExecutorPolicy, FixedTime, FixedUpdate, Query};

const ARENA: i64 = 4096;
const REPEATS: usize = 2;

#[derive(Component, Debug, PartialEq)]
struct Identity(u32);
#[derive(Component, Debug, PartialEq)]
struct Position {
    x: i64,
    y: i64,
}
#[derive(Component, Debug, PartialEq)]
struct Velocity {
    x: i64,
    y: i64,
}
#[derive(Component, Debug, PartialEq)]
struct Weapon {
    phase: u64,
    period: u64,
    shots: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Config {
    entities: u32,
    steps: u64,
    threads: NonZeroUsize,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Config>, String> {
    let mut config = Config {
        entities: 10_000,
        steps: 120,
        threads: NonZeroUsize::MIN,
    };
    let mut args = args.into_iter();
    let mut seen_entities = false;
    let mut seen_steps = false;
    let mut seen_threads = false;
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            return Ok(None);
        }
        let seen = match flag.as_str() {
            "--entities" => &mut seen_entities,
            "--steps" => &mut seen_steps,
            "--threads" => &mut seen_threads,
            _ => return Err(format!("unknown argument: {flag}")),
        };
        if *seen {
            return Err(format!("duplicate argument: {flag}"));
        }
        *seen = true;
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
            "--threads" => {
                config.threads = value
                    .parse()
                    .map_err(|_| "--threads must be a positive integer")?;
            }
            _ => unreachable!(),
        }
    }
    Ok(Some(config))
}

fn patrol(mut query: Query<(&mut Position, &Velocity)>) {
    query.for_each(|_, (position, velocity)| {
        position.x = (position.x + velocity.x).rem_euclid(ARENA);
        position.y = (position.y + velocity.y).rem_euclid(ARENA);
    });
}

fn fire(mut query: Query<&mut Weapon>) {
    query.for_each(|_, weapon| {
        weapon.phase += 1;
        if weapon.phase == weapon.period {
            weapon.phase = 0;
            weapon.shots += 1;
        }
    });
}

fn setup(entities: u32) -> App {
    let mut app = App::new();
    for id in 0..entities {
        let n = i64::from(id);
        let period = 3 + u64::from(id % 13);
        app.world_mut().spawn_with((
            Identity(id),
            Position {
                x: (n * 37 + 11) % ARENA,
                y: (n * 53 + 7) % ARENA,
            },
            Velocity {
                x: n % 17 - 8,
                y: (n * 3) % 19 - 9,
            },
            Weapon {
                phase: u64::from(id) % period,
                period,
                shots: 0,
            },
        ));
    }
    app.add_systems(FixedUpdate, patrol);
    app.add_systems(FixedUpdate, fire);
    app
}

// This oracle never runs the systems: it derives each final component directly
// from the entity index and tick count using wider closed-form arithmetic.
fn validate(app: &App, config: Config) -> Result<u64, String> {
    let world = app.world();
    if world.entity_count() != config.entities as usize {
        return Err("entity count mismatch".into());
    }
    if world
        .resource::<FixedTime>()
        .ok_or("missing FixedTime")?
        .tick()
        != config.steps
    {
        return Err("fixed tick mismatch".into());
    }
    let mut checksum = 0xcbf29ce484222325_u64;
    for (expected_index, entity) in world.entities().enumerate() {
        if entity.index() as usize != expected_index || entity.generation() != 0 {
            return Err(format!("unexpected entity handle: {entity:?}"));
        }
        let identity = world.get::<Identity>(entity).ok_or("missing Identity")?;
        let position = world.get::<Position>(entity).ok_or("missing Position")?;
        let velocity = world.get::<Velocity>(entity).ok_or("missing Velocity")?;
        let weapon = world.get::<Weapon>(entity).ok_or("missing Weapon")?;
        let n = i128::from(entity.index());
        let ticks = i128::from(config.steps);
        let vx = n % 17 - 8;
        let vy = (n * 3) % 19 - 9;
        let period = 3 + n % 13;
        let accumulated_phase = n % period + ticks;
        let expected_position = Position {
            x: (37 * n + 11 + ticks * vx).rem_euclid(i128::from(ARENA)) as i64,
            y: (53 * n + 7 + ticks * vy).rem_euclid(i128::from(ARENA)) as i64,
        };
        let expected_velocity = Velocity {
            x: vx as i64,
            y: vy as i64,
        };
        let expected_weapon = Weapon {
            phase: (accumulated_phase % period) as u64,
            period: period as u64,
            shots: (accumulated_phase / period) as u64,
        };
        if identity.0 != entity.index()
            || *position != expected_position
            || *velocity != expected_velocity
            || *weapon != expected_weapon
        {
            return Err(format!(
                "state mismatch for entity {entity:?}: {identity:?}, {position:?}, {velocity:?}, {weapon:?}"
            ));
        }
        // Explicit little-endian encoding keeps the diagnostic checksum portable.
        for value in [
            u64::from(identity.0),
            position.x as u64,
            position.y as u64,
            velocity.x as u64,
            velocity.y as u64,
            weapon.phase,
            weapon.period,
            weapon.shots,
        ] {
            for byte in value.to_le_bytes() {
                checksum ^= u64::from(byte);
                checksum = checksum.wrapping_mul(0x100000001b3);
            }
        }
    }
    Ok(checksum)
}

#[derive(serde::Serialize)]
struct Timings {
    initialization_ns: u64,
    simulation_ns: u64,
    validation_ns: u64,
}

fn elapsed_ns(start: Instant) -> Result<u64, String> {
    start
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| "timing exceeds u64 nanoseconds".into())
}

fn run_once(config: Config) -> Result<(u64, Timings), String> {
    let start = Instant::now();
    let mut app = setup(config.entities);
    if config.threads.get() > 1 {
        app.set_executor_policy(ExecutorPolicy::Parallel {
            max_threads: config.threads,
        });
    }
    let initialization_ns = elapsed_ns(start)?;
    let start = Instant::now();
    app.try_advance_fixed(config.steps)
        .map_err(|errors| format!("simulation failed: {errors:?}"))?;
    let simulation_ns = elapsed_ns(start)?;
    let start = Instant::now();
    let checksum = validate(&app, config)?;
    let validation_ns = elapsed_ns(start)?;
    Ok((
        checksum,
        Timings {
            initialization_ns,
            simulation_ns,
            validation_ns,
        },
    ))
}

fn execute(config: Config) -> Result<serde_json::Value, String> {
    let mut checksum = None;
    let mut runs = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let (actual, timings) = run_once(config)?;
        if checksum.is_some_and(|expected| expected != actual) {
            return Err("repeat checksum mismatch".into());
        }
        checksum = Some(actual);
        runs.push(timings);
    }
    let bytes_per_entity =
        size_of::<Identity>() + size_of::<Position>() + size_of::<Velocity>() + size_of::<Weapon>();
    Ok(serde_json::json!({
        "schema_version": 1, "workload": "swarm", "entities": config.entities,
        "steps": config.steps, "executor": if config.threads.get() == 1 { "sequential" } else { "parallel" },
        "max_threads": config.threads.get(), "repeats": REPEATS, "checksum": format!("{:016x}", checksum.unwrap()),
        "correctness": {"expected_state": true, "repeat_agreement": true}, "runs": runs,
        "memory": {"bytes_per_entity": bytes_per_entity, "logical_component_payload_bytes": u64::from(config.entities) * bytes_per_entity as u64},
        "environment": {"os": env::consts::OS, "arch": env::consts::ARCH, "debug_assertions": cfg!(debug_assertions)}
    }))
}

fn main() -> ExitCode {
    let result = parse_args(env::args().skip(1)).and_then(|config| match config {
        Some(config) => execute(config).map(|report| println!("{report}")),
        None => { println!("Usage: swarm [--entities N] [--steps N] [--threads N]\nDefaults: --entities 10000 --steps 120 --threads 1. Threads above 1 opt into parallel execution. Each invocation validates two fresh runs."); Ok(()) }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("swarm: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_form_matches_wrapping_and_firing_across_sizes() {
        for entities in [0, 1, 17, 257] {
            for steps in [0, 1, 15, 120, 513] {
                let config = Config {
                    entities,
                    steps,
                    threads: NonZeroUsize::MIN,
                };
                let first = run_once(config).unwrap().0;
                assert_eq!(first, run_once(config).unwrap().0);
            }
        }
    }

    #[test]
    fn parallel_matches_sequential_oracle_and_checksums() {
        for entities in [0, 1, 257, 10000] {
            for steps in [0, 1, 120] {
                let mut config = Config {
                    entities,
                    steps,
                    threads: NonZeroUsize::MIN,
                };
                let sequential = run_once(config).unwrap().0;
                config.threads = NonZeroUsize::new(2).unwrap();
                assert_eq!(run_once(config).unwrap().0, sequential);
                assert_eq!(run_once(config).unwrap().0, sequential);
            }
        }
    }

    #[test]
    fn oracle_rejects_corruption_and_missing_entities() {
        let config = Config {
            entities: 3,
            steps: 20,
            threads: NonZeroUsize::MIN,
        };
        let mut app = setup(config.entities);
        app.try_advance_fixed(config.steps).unwrap();
        let entity = app.world().entities().next().unwrap();
        assert!(validate(&app, config).is_ok());
        app.world_mut().get_mut::<Position>(entity).unwrap().x += 1;
        assert!(validate(&app, config).is_err());
        app.world_mut().get_mut::<Position>(entity).unwrap().x -= 1;
        app.world_mut().get_mut::<Weapon>(entity).unwrap().shots += 1;
        assert!(validate(&app, config).is_err());
        app.world_mut().get_mut::<Weapon>(entity).unwrap().shots -= 1;
        app.world_mut().get_mut::<Velocity>(entity).unwrap().y += 1;
        assert!(validate(&app, config).is_err());
        app.world_mut().get_mut::<Velocity>(entity).unwrap().y -= 1;
        app.world_mut().get_mut::<Identity>(entity).unwrap().0 += 1;
        assert!(validate(&app, config).is_err());
        app.world_mut().get_mut::<Identity>(entity).unwrap().0 -= 1;
        assert!(validate(&app, config).is_ok());
        app.world_mut().despawn(entity);
        assert!(validate(&app, config).is_err());
    }

    #[test]
    fn known_two_patrols_after_one_tick() {
        // Hand-calculated rows: (0,3,4094,-8,-9,1,3,0) and
        // (1,41,54,-7,-6,2,4,0), encoded as little-endian u64 values.
        let config = Config {
            entities: 2,
            steps: 1,
            threads: NonZeroUsize::MIN,
        };
        assert_eq!(run_once(config).unwrap().0, 0x71ac72405e5d407b);
        let app = setup(0);
        assert!(
            validate(
                &app,
                Config {
                    entities: 0,
                    steps: 1,
                    threads: NonZeroUsize::MIN,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn parser_rejects_invalid_configuration() {
        for args in [
            vec!["--entities"],
            vec!["--entities", "-1"],
            vec!["--entities", "4294967295"],
            vec!["--steps", "18446744073709551616"],
            vec!["--steps", "1", "--steps", "2"],
            vec!["--wat"],
            vec!["--threads", "0"],
            vec!["--threads", "2", "--threads", "3"],
        ] {
            assert!(parse_args(args.into_iter().map(str::to_owned)).is_err());
        }
        assert_eq!(
            parse_args(["--entities", "0", "--steps", "0"].map(str::to_owned)).unwrap(),
            Some(Config {
                entities: 0,
                steps: 0,
                threads: NonZeroUsize::MIN,
            })
        );
    }
}
