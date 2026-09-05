# Mixed-schedule executor overhead

The `mixed_schedule` example is a bounded measurement fixture for the existing
opt-in executor. It does not change executor behavior, scheduling order, or the
sequential default. It complements the preserved [swarm baseline](swarm.md) by
measuring four fixed schedule shapes:

- `small_compatible`: four independent component writers with very little work;
- `uneven_compatible`: four independent writers with relative work of 1:8:1:4;
- `conflicts`: two writes and a read of one component around an independent write;
- `commands_barriers`: typed work, Commands, ApplyDeferred, then two compatible
  typed systems.

Every process constructs two fresh worlds for every scenario. Final components,
the conflict observer, spawned entity count and spawned values are checked using
independently calculated expected state. Checksums must agree between the fresh
worlds and, when comparing reports, across executor policies.

## Run the comparison

```sh
python3 scripts/measure-mixed-schedules.py \
  --counts 0 64 1000 10000 --steps 120 --work-iterations 16 \
  --threads 1 2 4 8 --repeats 3 \
  --environment-notes 'record load, power mode, and concurrent work here' \
  > /tmp/mixed-schedules.json
```

The runner builds once in release mode and launches a fresh child for each
entity-count, thread-limit, and repetition combination. `--threads 1` preserves
the sequential policy; larger values opt into `ExecutorPolicy::Parallel`.
`--debug` is intended only for the bounded CI smoke gate. macOS and Linux report
whole-child peak RSS through `wait4`; the Rust example itself remains usable on
other native platforms.

Reports capture the revision and dirty state, Rust compiler, target platform,
logical CPU count, `RUSTFLAGS`, load average before and after the sample set and
before each child, plus the CPU model and free-form environment notes. Repetition
order rotates the thread limits to reduce systematic thermal and load bias.
Record power mode and other active work in the notes because load average alone
cannot characterize CPU contention or frequency changes.

## Reading the evidence

Each scenario reports initialization, simulation, and validation nanoseconds.
Simulation includes fixed stepping and schedule execution. It excludes setup and
the independent state scan, but is not a per-system profiler. The process wall
time and peak RSS include both fresh worlds for all four scenarios.

Entity count zero is the no-query-work calibration: the typed callbacks still
run with the same access metadata and batch shape, so its parallel-minus-
sequential delta estimates the combined planner, context-preparation,
scoped-spawn, and join envelope. Instrumenting those phases separately would
perturb this short path and is outside the approved investigation.

The `schedule` object makes the fixture's preparation and dispatch exposure
explicit. `batch_sizes` applies the documented contiguous compatibility and
thread-limit rules to this fixed, known schedule. The other fields count prepared
batches, prepared contexts, worker dispatches, singleton callbacks, conflict
splits, and command/deferred barriers per fixed tick. These are structural
counts, not internal timers or new executor instrumentation. For example, limit two
produces `[2,2]` for the compatible schedules, `[1,2,1]` for conflicts, and
`[1,1,1,2]` around command barriers. The example tests these shapes so a fixture
edit cannot silently relabel the evidence. The fresh-process runner verifies that
the shapes stay invariant and hoists them into `schedule_shapes_by_thread_limit`
instead of repeating identical metadata in every raw sample.

Compare medians and ranges from repeated release samples on the same machine and
toolchain; do not turn local results into a CI timing threshold. In particular:

- small compatible work exposes scoped-thread preparation and dispatch cost;
- uneven work shows whether a contiguous batch is limited by its longest member;
- conflicts show how singleton work reduces the useful parallel fraction;
- command barriers include unavoidable serial reservation/application work.

## Recorded evidence

Measured 2026-09-05 at 15:46 UTC on macOS 27 arm64, an 18-logical-CPU Apple M5
Pro, with rustc 1.98.1 and the release profile. The machine was connected to AC
power. Normal interactive desktop, UI, and media-analysis work remained active;
load averages moved from 6.93/7.09/6.28 to 6.93/7.08/6.30, so the ranges below
matter and the results are not an isolated-machine benchmark.

The [raw report](evidence/executor-mixed.json) records clean revision
`1be3b69e0a1b8b5d2033b5a2dd26de675e3a64fc`, five fresh child processes per
count/policy and two fresh worlds per scenario in each child. Every expected-state
check passed, repeat checksums agreed, and checksums matched across policies.
Cells are the median and full range of ten simulation samples, in milliseconds
for 120 fixed ticks. Initialization, validation, whole-process wall time, RSS,
sample order, load, and exact batch shapes remain in the raw report.

| Scenario | Entities | Sequential | Limit 2 | Limit 4 | Limit 8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Small compatible | 0 | 0.011 (0.010–0.012) | 7.034 (6.081–9.259) | 5.492 (4.526–6.464) | 5.191 (4.614–5.830) |
| Small compatible | 64 | 0.079 (0.073–0.085) | 6.380 (6.109–6.559) | 5.267 (4.627–5.975) | 5.232 (4.783–5.630) |
| Small compatible | 1,000 | 0.724 (0.676–0.746) | 6.862 (6.207–7.044) | 5.517 (5.128–5.909) | 5.167 (4.800–5.567) |
| Small compatible | 10,000 | 9.221 (6.464–10.159) | 13.710 (12.914–14.784) | 8.596 (7.540–9.060) | 8.670 (8.492–9.732) |
| Uneven compatible | 0 | 0.028 (0.025–0.033) | 6.885 (5.950–8.974) | 5.334 (4.592–9.376) | 4.904 (4.599–5.983) |
| Uneven compatible | 64 | 1.546 (1.528–1.577) | 7.253 (6.960–7.777) | 5.729 (4.974–6.561) | 5.420 (5.143–6.079) |
| Uneven compatible | 1,000 | 23.397 (23.299–23.652) | 28.010 (27.771–28.736) | 20.449 (19.767–20.819) | 20.139 (19.872–21.135) |
| Uneven compatible | 10,000 | 234.274 (232.521–236.246) | 223.539 (221.315–229.710) | 160.967 (159.899–176.677) | 160.402 (160.070–161.115) |
| Conflicts | 0 | 0.014 (0.013–0.016) | 3.687 (3.002–3.964) | 3.465 (3.062–4.994) | 3.114 (2.946–3.600) |
| Conflicts | 64 | 0.087 (0.083–0.094) | 3.155 (3.077–3.689) | 3.285 (3.083–4.472) | 3.296 (3.075–3.505) |
| Conflicts | 1,000 | 0.816 (0.752–0.863) | 3.885 (3.681–4.196) | 3.895 (3.716–4.157) | 3.881 (3.790–4.592) |
| Conflicts | 10,000 | 10.419 (9.629–10.709) | 12.507 (12.398–12.745) | 12.688 (11.894–13.139) | 12.372 (11.778–12.891) |
| Commands barriers | 0 | 0.045 (0.039–0.062) | 3.581 (2.977–4.050) | 3.111 (2.940–4.468) | 3.077 (2.987–4.194) |
| Commands barriers | 64 | 0.079 (0.073–0.083) | 3.083 (3.023–3.503) | 3.221 (3.025–4.236) | 3.229 (3.064–3.388) |
| Commands barriers | 1,000 | 0.411 (0.383–0.441) | 3.762 (3.595–4.005) | 3.755 (3.533–4.363) | 3.766 (3.676–4.313) |
| Commands barriers | 10,000 | 4.817 (4.141–4.891) | 9.011 (8.383–9.272) | 8.828 (8.330–9.548) | 8.902 (8.457–9.247) |

For the zero-entity small-compatible calibration, subtracting sequential time
and dividing by prepared batches and ticks estimates a combined overhead of 29.3
µs per batch/tick at limit 2, 45.7 µs at limit 4, and 43.2 µs at limit 8. This
does not separate compatibility checks, context preparation, spawning, or joins.

Only the uneven workload shows a stable practical crossover in this matrix:
limits 4 and 8 are already modestly faster at 1,000 entities and about 31% faster
at 10,000. Limit 2 is slower at 1,000 and only about 5% faster at 10,000 because
the 8× lane dominates its pair. Small compatible work is near parity only at
10,000 with limits 4/8, while the conflict and Commands shapes remain slower at
every measured count. Raising the limit from 4 to 8 cannot widen these four-
system batches and provides no material median benefit.

The practical guidance remains opt-in: use sequential execution for small or
barrier-heavy schedules unless representative release measurements show a stable
benefit. Increase the limit only when sufficiently heavy compatible systems can
amortize preparation, spawning and joining, and do not choose a limit above the
useful contiguous batch width. This evidence does not
justify a default change, persistent worker pool, work stealing, intra-query
parallelism, or another optimization; any such change needs separately approved
scope and before/after measurement.
