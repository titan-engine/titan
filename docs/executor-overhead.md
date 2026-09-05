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
edit cannot silently relabel the evidence.

Compare medians and ranges from repeated release samples on the same machine and
toolchain; do not turn local results into a CI timing threshold. In particular:

- small compatible work exposes scoped-thread preparation and dispatch cost;
- uneven work shows whether a contiguous batch is limited by its longest member;
- conflicts show how singleton work reduces the useful parallel fraction;
- command barriers include unavoidable serial reservation/application work.

The practical guidance remains opt-in: use sequential execution for small or
barrier-heavy schedules unless representative release measurements show a stable
benefit. Increase the limit only when compatible systems have enough balanced
work to amortize preparation, spawning and joining. This evidence does not
justify a default change, persistent worker pool, work stealing, intra-query
parallelism, or another optimization; any such change needs separately approved
scope and before/after measurement.
