# Sparse component retention under entity churn

The headless `sparse_churn` fixture measures the cost of sparse component
placement and repeated entity reuse for [issue #71](https://github.com/titan-engine/titan/issues/71).
It complements the [steady swarm](swarm.md) experiment. It does not run a
scheduler, render, or change the storage policy.

```sh
cargo run --release --example sparse_churn -- --distribution rare-high --entities 100000 --cycles 10
python3 scripts/measure-sparse-churn.py --counts 1000 100000 --cycles 10 --repeats 3 > /tmp/sparse-churn.json
# Small deterministic CI run, with no timing or memory thresholds:
python3 scripts/measure-sparse-churn.py --debug --counts 1 32 --cycles 3 --repeats 2
```

The Python runner builds once and runs each count/distribution/repetition in a
fresh child process. It supports macOS and Linux; the Rust fixture can run on
other native platforms. Defaults are release, all three distributions, ten churn
cycles, and three repetitions. Counts are limited to 1–1,000,000, cycles to
1–100, and count × cycles to 10,000,000. Repeats are limited to 1–20 and a runner
invocation to 180 children. Each child has a wall deadline of 120 seconds by
default (`--timeout-seconds`, 1–600). Compilation uses the existing
[acceptance build deadline](acceptance-timeouts.md). These bounds constrain the
experiment, not engine capacity or a guaranteed allocation budget.

## Workload and semantic verification

Each process uses one world with a 16-byte component containing index and epoch. Dense attaches it to
every entity; rare-low and rare-high attach it to the lowest and highest
`max(1, entities / 100)` allocator indices, respectively. Rare-high first attaches
the component near the top of the allocated index range, exposing the sparse
index allocation even though very few values exist.

The fixture records the empty world, spawned entities, attached components,
mass despawn, entity reuse, reattachment, repeated churn, and final despawn.
Validation runs outside the operation timers and checks live identities,
component membership and values, query results, stale handles, and recycled
indices with advanced generations. The runner compares semantic checksums across
repetitions and fails on disagreement. Rust tests and the process smoke run run
in CI; failures do not become successful measurement reports.

## What the numbers mean

- `logical_payload_bytes` counts live component values times their Rust size.
  It excludes dense entity IDs, sparse indices, spare capacity, and bookkeeping.
- Snapshot storage statistics report actual `Vec` lengths, capacities, element
  sizes, and capacity × element-size bytes for the entity allocator and component
  sparse/dense vectors. `World::storage_stats()` is read-only and sorted by
  component type name. These are retained vector allocation sizes, not total heap
  use. They exclude vector/box headers, hash maps, resources, deferred commands,
  heap allocations owned by components, allocator bookkeeping and rounding.
  Zero-sized values contribute zero bytes. Layout and growth are implementation
  details, not API capacity guarantees.
- `peak_process_rss_bytes` is the fresh child's whole-lifetime high-water RSS
  from `wait4` (macOS bytes; Linux KiB converted to bytes). It includes validation,
  fixture bookkeeping, snapshot/report allocations, runtime and teardown. It
  excludes Cargo and Python. It cannot locate a phase peak, demonstrate that
  freed memory returned to the OS, or be subtracted from payload to obtain ECS
  overhead.
- Rust operation timers are wall durations. `spawn` includes pushing fixture
  handles; `attach`/`reattach` scan all indices to select membership, so rare
  attachment time includes that scan. `despawn` sums the initial and final mass
  despawns; `reuse` is the first refill, and `churn` sums all additional cycles
  of despawn, refill and attachment. Validation and storage snapshots are
  outside them. Every `churn_N` snapshot records the corresponding live cycle. `process_wall_seconds` includes launch, checks and output; the
  runner polls at approximately 10 ms, so use Rust timers for short operations.

The report records source revision and dirty status, UTC time, Rust compiler,
platform/architecture, logical CPU count, profile, `RUSTFLAGS`, and all workload
parameters. Compare repeated release runs on the same environment. Power mode,
background load, other Cargo configuration and allocator behavior can affect the
observations; there are no machine-dependent CI performance budgets.
