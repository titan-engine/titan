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

## Recorded release evidence

Measured 2026-09-05 at 16:28 UTC on macOS 27.0 arm64, 18 logical CPUs,
Rust 1.98.1 (48a229cea), release profile, empty `RUSTFLAGS`, ten churn cycles.
The [raw report](evidence/sparse-churn-baseline.json) records clean source commit
`22fa36f7a3d9c03644ea0cec2641268328c35f5d`. Three fresh processes for each of six
configurations all passed the semantic checks and agreed on checksums. Local
workspace verification was running concurrently; power mode and background load
were not controlled. Treat timing/RSS as local observations.

All sizes below are bytes. Component capacity sums sparse entries, dense entity
IDs and values after initial attachment. Final ECS capacity adds allocator slots
and free indices after the final despawn. Final logical payload is zero in every
sample; the component vector capacities remain identical to the attached snapshot
through reattachment, every churn cycle, and final despawn.

| Entities | Distribution | Live payload | Sparse capacity | Component capacity | Final ECS capacity | Peak process RSS range |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1,000 | dense | 16,000 | 24,576 | 49,152 | 61,440 | 2,375,680–2,375,680 |
| 1,000 | rare-low | 160 | 384 | 768 | 13,056 | 2,310,144–2,310,144 |
| 1,000 | rare-high | 160 | 47,568 | 47,952 | 60,240 | 2,392,064–2,392,064 |
| 100,000 | dense | 1,600,000 | 3,145,728 | 6,291,456 | 7,864,320 | 12,042,240–12,255,232 |
| 100,000 | rare-low | 16,000 | 24,576 | 49,152 | 1,622,016 | 5,357,568–5,373,952 |
| 100,000 | rare-high | 16,000 | 4,752,048 | 4,776,624 | 6,349,488 | 7,733,248–7,749,632 |

Median operation wall times in milliseconds across the three repetitions:

| Entities | Distribution | Spawn | Attach | Two mass despawns | First reuse | Reattach | Ten churn cycles | Validation |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | dense | 0.005 | 0.023 | 0.010 | 0.002 | 0.009 | 0.145 | 0.317 |
| 1,000 | rare-low | 0.005 | 0.002 | 0.010 | 0.002 | 0.001 | 0.053 | 0.306 |
| 1,000 | rare-high | 0.005 | 0.011 | 0.010 | 0.002 | 0.001 | 0.059 | 0.356 |
| 100,000 | dense | 0.264 | 1.462 | 1.042 | 0.160 | 1.168 | 17.057 | 35.514 |
| 100,000 | rare-low | 0.268 | 0.056 | 0.625 | 0.163 | 0.059 | 5.218 | 30.129 |
| 100,000 | rare-high | 0.267 | 0.218 | 0.671 | 0.157 | 0.057 | 5.235 | 30.599 |

## Interpretation and follow-up decision

Sparse storage grows to cover the highest attached entity index, independently
of component density. At 100,000 entities the two rare cases have the same
16,000-byte payload, but rare-high retains 4,752,048 sparse bytes versus 24,576
for rare-low. Here `Option<SparseEntry>` is 24 bytes. The first rare-high insert
resizes to 99,001 entries; a subsequent insert crosses that capacity and observed
vector growth doubles it to 198,002. Dense gradual growth reaches 131,072 entries.
This explains why rare-high can retain more sparse bytes than dense even with
only one percent as many values. The growth factors are observations of this
Rust allocator/vector implementation, not portable requirements.

Despawn clears membership and removes dense values but does not shrink vectors.
Allocator slots also remain, and freed indices occupy a retained free list.
Reuse advances generations without increasing the index range. Across these ten
bounded cycles the retained capacities plateau rather than grow per cycle. This
is evidence of reusable high-water storage, not evidence that process RSS falls
on despawn or that every possible workload is leak-free. Dropping a world releases
its owned allocations to the allocator; the OS may still retain those pages.

A bounded follow-up investigation is warranted if large worlds use many rare
component types: compare a paged sparse index against this high-index fixture,
including dense-operation costs, before selecting a storage change. The measured
amplification is enough to motivate that comparison, but does not establish that
replacement would improve a real game's overall tradeoff. No optimization or
follow-up implementation is approved by this report; issue #71 owns this completed
investigation and subsequent scope remains a maintainer decision. There is no
basis here for changing the scheduler or promising million-entity capacity.
