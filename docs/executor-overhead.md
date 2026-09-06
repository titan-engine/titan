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
instead of repeating identical metadata in every raw sample. It also enforces and
hoists each scenario checksum by entity count, so any world, outer repetition, or
executor policy divergence aborts the report instead of publishing inconsistent
evidence.

Compare medians and ranges from repeated release samples on the same machine and
toolchain; do not turn local results into a CI timing threshold. In particular:

- small compatible work exposes scoped-thread preparation and dispatch cost;
- uneven work shows whether a contiguous batch is limited by its longest member;
- conflicts show how singleton work reduces the useful parallel fraction;
- command barriers include unavoidable serial reservation/application work.

## Recorded evidence

Measured 2026-09-05 at 15:54 UTC on macOS 27 arm64, an 18-logical-CPU Apple M5
Pro, with rustc 1.98.1 and the release profile. The machine was connected to AC
power. Normal interactive desktop, UI, and media-analysis work remained active;
load averages moved from 5.30/5.78/5.92 to 6.05/5.91/5.96, so the ranges below
matter and the results are not an isolated-machine benchmark.

The [raw report](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/evidence/executor-mixed.json) records clean revision
`d16a700f0801661c20eeedf3ab94294217f3a179`, five fresh child processes per
count/policy and two fresh worlds per scenario in each child. Every expected-state
check passed, repeat checksums agreed, and checksums matched across policies.
Cells are the median and full range of ten simulation samples, in milliseconds
for 120 fixed ticks. Initialization, validation, whole-process wall time, RSS,
sample order, load, and exact batch shapes remain in the raw report.

| Scenario | Entities | Sequential | Limit 2 | Limit 4 | Limit 8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Small compatible | 0 | 0.011 (0.009–0.018) | 6.928 (6.118–11.404) | 5.468 (5.027–6.347) | 5.120 (4.937–6.066) |
| Small compatible | 64 | 0.074 (0.068–0.085) | 6.844 (6.406–8.893) | 5.350 (4.898–5.713) | 5.594 (4.742–6.072) |
| Small compatible | 1,000 | 0.690 (0.643–0.759) | 7.344 (6.930–8.893) | 5.466 (5.245–6.066) | 5.918 (5.189–6.474) |
| Small compatible | 10,000 | 8.958 (7.744–10.127) | 15.085 (13.238–16.818) | 9.026 (8.059–10.246) | 9.026 (8.694–9.528) |
| Uneven compatible | 0 | 0.027 (0.024–0.047) | 6.717 (6.354–9.486) | 5.489 (4.899–6.054) | 5.270 (5.038–5.906) |
| Uneven compatible | 64 | 1.529 (1.461–1.649) | 8.018 (7.639–9.777) | 5.696 (5.434–6.605) | 6.157 (5.533–6.433) |
| Uneven compatible | 1,000 | 22.672 (22.307–23.323) | 29.987 (29.084–30.481) | 20.551 (20.294–21.368) | 20.821 (20.370–21.472) |
| Uneven compatible | 10,000 | 225.837 (221.947–242.979) | 218.765 (216.451–260.575) | 159.492 (154.161–182.604) | 156.151 (154.763–162.583) |
| Conflicts | 0 | 0.013 (0.012–0.022) | 3.442 (3.219–4.539) | 3.304 (3.163–4.078) | 3.285 (2.913–3.890) |
| Conflicts | 64 | 0.086 (0.081–0.100) | 3.512 (3.323–4.503) | 3.464 (3.183–3.539) | 3.552 (3.448–4.195) |
| Conflicts | 1,000 | 0.783 (0.730–0.815) | 4.298 (4.038–5.010) | 4.358 (4.123–5.245) | 4.254 (4.096–4.938) |
| Conflicts | 10,000 | 10.037 (9.921–10.611) | 13.103 (12.790–13.296) | 13.205 (12.695–13.750) | 13.149 (12.679–13.840) |
| Commands barriers | 0 | 0.043 (0.039–0.079) | 3.373 (3.107–6.212) | 3.341 (3.121–3.917) | 3.400 (3.048–4.291) |
| Commands barriers | 64 | 0.076 (0.071–0.090) | 3.467 (3.289–4.547) | 3.452 (3.099–3.682) | 3.476 (3.375–4.278) |
| Commands barriers | 1,000 | 0.387 (0.351–0.425) | 4.176 (3.936–4.590) | 4.065 (3.795–4.846) | 4.039 (3.881–4.649) |
| Commands barriers | 10,000 | 4.623 (4.343–4.851) | 9.016 (8.697–9.512) | 9.158 (8.698–9.732) | 9.011 (8.668–9.410) |

For the zero-entity small-compatible calibration, subtracting sequential time
and dividing by prepared batches and ticks estimates a combined overhead of 28.8
µs per batch/tick at limit 2, 45.5 µs at limit 4, and 42.6 µs at limit 8. This
does not separate compatibility checks, context preparation, spawning, or joins.

Only the uneven workload shows a stable practical crossover in this matrix:
limits 4 and 8 are already modestly faster at 1,000 entities and about 30% faster
at 10,000. Limit 2 is slower at 1,000 and only about 3% faster at 10,000 because
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
parallelism, or another optimization; any such change needs a separate issue with concrete
scope and before/after measurement.
