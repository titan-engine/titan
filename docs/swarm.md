# Deterministic swarm workload

The headless `swarm` example exercises Titan's existing sequential ECS schedule
with configurable counts of unnamed moving entities and periodic weapon firing.
It supplies a reproducible game-driven baseline for evaluating future execution
changes. It does not implement a scheduler or establish a capacity guarantee.

```sh
cargo run --release --example swarm -- --entities 10000 --steps 120
python3 scripts/measure-swarm.py --counts 1000 10000 --steps 120 --repeats 3 > /tmp/swarm.json
```

The runner builds once, then launches a fresh process for each count/repetition.
Use `--debug` for quick smoke tests. `--timeout-seconds` bounds each measured
process (default 120 seconds), excluding compilation. Counts and steps may be
zero for boundary checks; repetitions must be positive. Very large inputs remain
subject to available memory and execution time. The Rust example can run directly
on other native platforms; the RSS runner supports macOS and Linux.

## Workload and correctness

Each entity has identity, position, velocity and weapon components. Fixed integer
arithmetic defines initial state, toroidal movement and periodic firing. Two typed
systems execute in registration order using the existing fixed-update schedule.
This deliberately isolates dense component joins and independent component updates; it does not
represent rendering, collision/neighbour searches, structural churn, or a complete
game. Entity counts vary the workload size while preserving its behavior.

Each example invocation constructs and runs two fresh worlds. It checks every
entity's component state against an independent closed-form oracle, then compares
canonical final-state checksums between the worlds. A successful JSON report has
`correctness.expected_state` and `correctness.repeat_agreement` set to true.
A failed check exits unsuccessfully; the runner propagates failures rather than
publishing a successful sample. Automated tests run with `cargo test --workspace
--all-targets`; CI also exercises the actual process-measurement runner.

## Interpreting measurements

The example reports nanoseconds separately for initialization, simulation and
validation in each of its two runs. Simulation timing includes schedule execution
and fixed stepping, and excludes initialization and the final correctness scan.
These are aggregate wall durations, not per-system profiles or a steady-state
latency distribution. The first run is not discarded as warmup. Teardown and JSON
serialization are outside these phase timers.

`memory.logical_component_payload_bytes` is component sizes times entity count.
It excludes sparse/dense storage capacity, entity bookkeeping, resources, allocator
overhead and executable/runtime memory; it is not allocated heap or resident
memory. The measurement runner adds `peak_process_rss_bytes`, obtained with
`wait4` for that specific child, converting Linux KiB to bytes (macOS already
reports bytes). This is the whole-process high-water mark across both simulations,
setup, verification and teardown. It includes retained allocator pages and runtime
memory, excludes Cargo/Python, and cannot isolate the simulation's working set.

`process_wall_seconds` also includes launch and correctness checks; the runner
polls at approximately 10 ms, so use the Rust phase timers for short simulations.
The report records UTC time, revision, dirty-tree status, platform, architecture,
CPU count, Rust compiler, build profile and `RUSTFLAGS`. Record any additional
Cargo configuration, machine load and power mode when comparing environments.

Compare several release samples on the same machine and toolchain. Checksums and
oracle results establish correctness; timings and RSS are observations without
pass/fail thresholds. Do not infer million-entity support or scheduler speedups
from these baselines. Scheduler work remains separately scoped in issue #6.

## Recorded baseline

Measured 2026-09-05T09:56:08.655556+00:00 on macOS-27.0-arm64-arm-64bit with 18 logical CPUs,
Rust rustc 1.98.1 (48a229cea 2026-09-01), release profile, 120 steps.
The [raw report](evidence/swarm-baseline.json) records the clean measured
implementation revision `f96d98590089a75f6bfbb132a242490404b19f8e`.
Three fresh processes per size each execute two simulations. The table takes
the median of six simulation durations and the range of three peak RSS values;
these are local observations with no performance budget.

| Entities | Simulation median (ms / 120 steps) | Peak process RSS range (bytes) | Logical payload (bytes) | Checksum |
| ---: | ---: | ---: | ---: | --- |
| 1,000 | 0.815 | 2,719,744–2,736,128 | 60,000 | `4003ab8d05979666` |
| 10,000 | 7.888 | 6,029,312–9,158,656 | 600,000 | `c958f2333dc726d9` |
| 100,000 | 101.676 | 39,911,424–54,788,096 | 6,000,000 | `901b70e007e1a7f7` |
