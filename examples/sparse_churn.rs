//! Bounded sparse-component retention experiment. See docs/sparse-churn.md.
use serde::Serialize;
use std::{env, mem::size_of, process::ExitCode, time::Instant};
use titan::{
    Component,
    ecs::{Entity, World, WorldStorageStats},
};

#[derive(Component, Debug, PartialEq)]
struct Payload {
    index: u64,
    epoch: u64,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Distribution {
    Dense,
    RareLow,
    RareHigh,
}
#[derive(Clone, Copy, Debug, Serialize)]
struct Config {
    distribution: Distribution,
    entities: usize,
    cycles: usize,
}
impl Config {
    fn rare_count(self) -> usize {
        (self.entities / 100).max(1)
    }
    fn selected(self, index: usize) -> bool {
        match self.distribution {
            Distribution::Dense => true,
            Distribution::RareLow => index < self.rare_count(),
            Distribution::RareHigh => index >= self.entities - self.rare_count(),
        }
    }
}
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Config>, String> {
    let mut config = Config {
        distribution: Distribution::RareHigh,
        entities: 10_000,
        cycles: 5,
    };
    let mut args = args.into_iter();
    let mut seen = std::collections::HashSet::new();
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            return Ok(None);
        }
        if !seen.insert(flag.clone()) {
            return Err(format!("duplicate argument: {flag}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--distribution" => {
                config.distribution = match value.as_str() {
                    "dense" => Distribution::Dense,
                    "rare-low" => Distribution::RareLow,
                    "rare-high" => Distribution::RareHigh,
                    _ => return Err("invalid distribution".into()),
                }
            }
            "--entities" => config.entities = value.parse().map_err(|_| "invalid entities")?,
            "--cycles" => config.cycles = value.parse().map_err(|_| "invalid cycles")?,
            _ => return Err(format!("unknown argument: {flag}")),
        }
    }
    if !(1..=1_000_000).contains(&config.entities)
        || !(1..=100).contains(&config.cycles)
        || config.entities * config.cycles > 10_000_000
    {
        return Err(
            "require entities 1..1000000, cycles 1..100 and entities*cycles <= 10000000".into(),
        );
    }
    Ok(Some(config))
}
#[derive(Serialize)]
struct Snapshot {
    phase: String,
    live_entities: usize,
    logical_payload_bytes: usize,
    storage: WorldStorageStats,
}
fn snapshot(world: &World, phase: impl Into<String>) -> Snapshot {
    Snapshot {
        phase: phase.into(),
        live_entities: world.entity_count(),
        logical_payload_bytes: world.iter::<Payload>().count() * size_of::<Payload>(),
        storage: world.storage_stats(),
    }
}
#[derive(Default, Serialize)]
struct Timings {
    spawn: f64,
    attach: f64,
    despawn: f64,
    reuse: f64,
    reattach: f64,
    churn: f64,
    validation: f64,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    config: Config,
    rare_count: usize,
    timings_ms: Timings,
    snapshots: Vec<Snapshot>,
    correctness: serde_json::Value,
    semantic_checksum: u64,
    memory_limitations: &'static str,
}
fn measure<T>(duration: &mut f64, action: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = action();
    *duration += start.elapsed().as_secs_f64() * 1000.0;
    result
}
fn attach(world: &mut World, handles: &[Entity], config: Config, epoch: u64) {
    // Index order fixes the first attachment, including the rare-high case after reuse.
    for (index, &entity) in handles.iter().enumerate() {
        if config.selected(index) {
            world
                .insert(
                    entity,
                    Payload {
                        index: index as u64,
                        epoch,
                    },
                )
                .unwrap();
        }
    }
}
fn validate(world: &World, handles: &[Entity], config: Config, epoch: u64, attached: bool) -> u64 {
    assert_eq!(world.entity_count(), config.entities);
    let mut expected_count = 0;
    let mut checksum = 0_u64;
    for (index, &entity) in handles.iter().enumerate() {
        assert_eq!(entity.index() as usize, index);
        assert!(world.is_alive(entity));
        let expected = (attached && config.selected(index)).then_some(Payload {
            index: index as u64,
            epoch,
        });
        assert_eq!(world.get::<Payload>(entity), expected.as_ref());
        if let Some(value) = expected {
            expected_count += 1;
            checksum = checksum.wrapping_add(value.index.wrapping_mul(31) + value.epoch);
        }
    }
    let mut visited = vec![false; config.entities];
    for (entity, value) in world.iter::<Payload>() {
        let index = entity.index() as usize;
        assert!(!visited[index]);
        visited[index] = true;
        assert_eq!(entity, handles[index]);
        assert!(attached && config.selected(index));
        assert_eq!(
            value,
            &Payload {
                index: index as u64,
                epoch
            }
        );
    }
    assert_eq!(visited.iter().filter(|&&v| v).count(), expected_count);
    checksum
}
fn despawn(world: &mut World, handles: &[Entity]) {
    for &entity in handles {
        assert!(world.despawn(entity));
    }
}
fn validate_dead(world: &mut World, handles: &[Entity]) {
    assert_eq!(world.entity_count(), 0);
    assert_eq!(world.entities().count(), 0);
    assert_eq!(world.iter::<Payload>().count(), 0);
    validate_stale(world, handles);
}
fn validate_stale(world: &mut World, handles: &[Entity]) {
    for &entity in handles {
        assert!(!world.is_alive(entity));
        assert!(world.get::<Payload>(entity).is_none());
        assert!(world.remove::<Payload>(entity).is_none());
        assert!(!world.despawn(entity));
        assert!(
            world
                .insert(entity, Payload { index: 0, epoch: 0 })
                .is_err()
        );
    }
}
fn reuse(world: &mut World, handles: &mut [Entity]) {
    // Release is ascending, reuse is LIFO. Keep handles indexed without sorting.
    for index in (0..handles.len()).rev() {
        handles[index] = world.spawn();
    }
}
fn validate_reuse(world: &mut World, old: &[Entity], handles: &[Entity]) {
    for (&before, &after) in old.iter().zip(handles) {
        assert_eq!(before.index(), after.index());
        assert_eq!(after.generation(), before.generation() + 1);
    }
    validate_stale(world, old);
}
fn run(config: Config) -> Report {
    let mut world = World::new();
    let mut times = Timings::default();
    let mut snapshots = vec![snapshot(&world, "empty")];
    let mut handles = Vec::with_capacity(config.entities);
    measure(&mut times.spawn, || {
        for _ in 0..config.entities {
            handles.push(world.spawn());
        }
    });
    snapshots.push(snapshot(&world, "spawned"));
    measure(&mut times.validation, || {
        validate(&world, &handles, config, 0, false)
    });
    measure(&mut times.attach, || {
        attach(&mut world, &handles, config, 0)
    });
    snapshots.push(snapshot(&world, "attached"));
    let mut checksum = measure(&mut times.validation, || {
        validate(&world, &handles, config, 0, true)
    });
    measure(&mut times.despawn, || despawn(&mut world, &handles));
    snapshots.push(snapshot(&world, "despawned"));
    measure(&mut times.validation, || {
        validate_dead(&mut world, &handles)
    });
    let mut old = handles.clone();
    measure(&mut times.reuse, || reuse(&mut world, &mut handles));
    snapshots.push(snapshot(&world, "reused"));
    measure(&mut times.validation, || {
        validate_reuse(&mut world, &old, &handles);
        validate(&world, &handles, config, 1, false);
    });
    measure(&mut times.reattach, || {
        attach(&mut world, &handles, config, 1)
    });
    snapshots.push(snapshot(&world, "reattached"));
    checksum = checksum.wrapping_add(measure(&mut times.validation, || {
        validate(&world, &handles, config, 1, true)
    }));
    for cycle in 0..config.cycles {
        old.copy_from_slice(&handles);
        measure(&mut times.churn, || {
            despawn(&mut world, &handles);
            reuse(&mut world, &mut handles);
            attach(&mut world, &handles, config, cycle as u64 + 2);
        });
        snapshots.push(snapshot(&world, format!("churn_{}", cycle + 1)));
        checksum = checksum.wrapping_add(measure(&mut times.validation, || {
            validate_reuse(&mut world, &old, &handles);
            validate(&world, &handles, config, cycle as u64 + 2, true)
        }));
    }
    measure(&mut times.despawn, || despawn(&mut world, &handles));
    snapshots.push(snapshot(&world, "final_despawned"));
    measure(&mut times.validation, || {
        validate_dead(&mut world, &handles)
    });
    Report {
        schema_version: 1,
        rare_count: config.rare_count(),
        config,
        timings_ms: times,
        snapshots,
        correctness: serde_json::json!({"membership_and_values": true, "stale_handles_rejected": true, "indices_reused_with_new_generation": true, "checksum": checksum}),
        semantic_checksum: checksum,
        memory_limitations: "Logical payload is live inline Payload bytes. Storage reports actual ECS vector capacities, excluding headers, hash maps, resources, deferred commands, allocator overhead and element-owned allocations. Process peak RSS is measured externally and also includes fixture handles, validation scratch space, snapshots, runtime and allocator retention; it is not phase RSS.",
    }
}
fn main() -> ExitCode {
    match parse_args(env::args().skip(1)) {
        Ok(Some(config)) => {
            println!("{}", serde_json::to_string(&run(config)).unwrap());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!(
                "sparse_churn --distribution dense|rare-low|rare-high --entities 1..1000000 --cycles 1..100 (entities*cycles <= 10000000)"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distributions_and_reuse_are_semantically_checked() {
        for entities in [1, 257] {
            for distribution in [
                Distribution::Dense,
                Distribution::RareLow,
                Distribution::RareHigh,
            ] {
                let config = Config {
                    distribution,
                    entities,
                    cycles: 3,
                };
                let report = run(config);
                assert_eq!(report.semantic_checksum, run(config).semantic_checksum);
                let attached = &report.snapshots[2];
                let final_state = report.snapshots.last().unwrap();
                let expected_count = match distribution {
                    Distribution::Dense => entities,
                    _ => (entities / 100).max(1),
                };
                assert_eq!(
                    attached.logical_payload_bytes,
                    expected_count * size_of::<Payload>()
                );
                let component = &attached.storage.components[0];
                let expected_len = if distribution == Distribution::RareLow {
                    config.rare_count()
                } else {
                    entities
                };
                assert_eq!(component.sparse.len, expected_len);
                assert_eq!(final_state.logical_payload_bytes, 0);
                assert_eq!(
                    final_state.storage.components[0].sparse.capacity_bytes,
                    component.sparse.capacity_bytes
                );
                assert_eq!(final_state.storage.components[0].values.len, 0);
                assert_eq!(
                    final_state.storage.components[0].values.capacity_bytes,
                    component.values.capacity_bytes
                );
            }
        }
    }
    #[test]
    fn rejects_unbounded_or_ambiguous_inputs() {
        for args in [
            vec!["--entities", "0"],
            vec!["--cycles", "101"],
            vec!["--entities", "1000001"],
            vec!["--entities", "1000000", "--cycles", "11"],
            vec!["--cycles", "1", "--cycles", "2"],
            vec!["--distribution", "other"],
        ] {
            assert!(parse_args(args.into_iter().map(String::from)).is_err());
        }
    }
}
