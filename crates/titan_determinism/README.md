# `titan_determinism`: find the first disagreeing tick

`titan_determinism` runs a fresh headless `titan_test::Sim` several times and
compares `titan_snapshot` world snapshots after every tick. If runs disagree, it
reports the earliest differing tick across all runs, with entity,
component/resource, and reflected field differences. Ties choose the lowest run
number. No window, renderer, GPU,
or live-game connection is required. Bevy's APIs and crate names are unchanged.

## Quick start

This complete example is also a runnable Rust doctest:

```rust
use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use titan_determinism::{DeterminismCheck, Variant};
use titan_test::Sim;

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Player {
    distance: u32,
}

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct Score {
    value: u32,
}

fn advance(mut players: Query<&mut Player>, mut score: ResMut<Score>) {
    for mut player in &mut players {
        player.distance += 1;
    }
    score.value += 1;
}

fn scenario() -> Sim {
    Sim::new(|app| {
        app.register_type::<Player>()
            .register_type::<Score>()
            .init_resource::<Score>()
            .add_systems(Update, advance);
        app.world_mut().spawn(Player { distance: 0 });
    })
    .with_fixed_dt(1.0 / 60.0)
    .with_seed(42)
}

for variant in [
    Variant::Repeat,
    Variant::MultiThreaded,
    Variant::ShuffleAmbiguous { seed: 123 },
] {
    let report = DeterminismCheck::new(scenario)
        .ticks(120)
        .runs(3)
        .variant(variant)
        .run();
    report.assert_deterministic();
}
```

From the Titan workspace:

```sh
cargo test -p titan_determinism --doc
cargo run -p titan_determinism --example hashmap_order
```

For your own game, depend on `titan_determinism` and `titan_test` as
dev-dependencies, replace `scenario` with your game setup, and keep presentation
plugins out of the simulation. Do not install `DefaultPlugins`: `Sim::new`
already installs its headless plugins. The factory must return a **fresh,
unticked** `Sim` on every call, with no gameplay state shared between runs.
`with_seed` supplies `SimSeed`; your game must use that resource to initialize its
own RNG. It does not seed global or third-party randomness automatically.

## What is compared?

- The main app world is captured **after each completed update**, including
  entity creation/removal, component presence, and captured resource values.
  Sub-app worlds are not compared, and the initial unticked world is not compared.
- Derive `Reflect` along with `Component` or `Resource`, add
  `#[reflect(Component)]` or `#[reflect(Resource)]`, and call
  `app.register_type::<YourType>()`. Deriving reflection alone is not enough.
  Registered, serializable values expose nested fields and array elements.
- Unregistered, non-reflectable, or unserializable values are **opaque**, not
  omitted. Their presence and opaque/reflected transitions can differ, but
  changes inside identical opaque markers cannot be detected. Reflection-ignored
  fields and non-send resources are not captured.
- `SnapshotConfig::default()` excludes derived `GlobalTransform`, generic
  `Time<*>` resources, and `AppTypeRegistry`. Use `.snapshot_config(config)` to
  change type-path allow/deny filters or `SnapshotConfig::all()` to remove those
  exclusions. Filters can hide meaningful differences.
- `.diff_config(DiffConfig { float_tolerance: 0.00001 })` permits an absolute,
  inclusive floating-point tolerance. The default is zero; integer pairs always
  compare exactly. Negative or non-finite tolerances are normalized to zero.

Entity comparison uses the **full index and generation**. Separate runs must
have identical allocation histories for IDs to identify corresponding entities;
there is no cross-run matching by name or gameplay identity. Different
allocation can produce additions/removals instead of the intended field diff.
Snapshots canonicalize reflected maps and sets, so mere iteration order is not a
difference; order-dependent *gameplay consequences* must reach observable state.

A passing check means the captured state agreed under these settings, not that
all internal state or every possible execution is deterministic. This checks
repeatability **within one process and build**, not across platforms, CPU
architectures, compiler versions, or builds. Rendering/GPU state, external
services, and hidden state are outside its observation boundary.

## Execution variants

`Variant::Repeat` is the default: every run keeps the factory's executor policy
and setup. It can catch unordered-collection gameplay, uncontrolled RNG,
wall-clock reads, async completion, and other hidden inputs when they affect
captured state during the chosen ticks.

`Variant::MultiThreaded` leaves run 1's executor policy unchanged and applies
`titan_test::ExecutorKind::MultiThreaded` to later runs. `Sim` normally uses
single-threaded schedules. This crate enables Bevy ECS's `multi_threaded` feature,
so the multithreaded executor is available, not a feature-disabled fallback.
Actual parallel execution still depends on task-pool workers and compatible
system accesses. It does not guarantee exploration of different ambiguous orders.

`Variant::ShuffleAmbiguous { seed }` leaves run 1 unchanged and chooses a
reproducible topological system order for subsequent runs, using a
**single-threaded executor** so conflicting unordered systems run in that order.
This can expose hidden dependencies without thread timing races. For example:

```rust
# use titan_determinism::{DeterminismCheck, Variant};
# use titan_test::Sim;
# fn scenario() -> Sim { Sim::new(|_| {}) }
let report = DeterminismCheck::new(scenario)
    .ticks(120)
    .variant(Variant::ShuffleAmbiguous { seed: 42 })
    .run();
println!("{report}");
```

The implementation uses Bevy's existing public
`ScheduleBuildSettings::shuffle_seed` hook, enabling `bevy_ecs/debug`. It shuffles
topological tie-breaking, not independent pairwise edges, so it cannot introduce
cycles or reverse explicit dependency paths (including weak-chain ordering of
conflicting systems). **All unordered systems may move**, not only pairs Bevy
reports as ambiguous; accepted/ignored ambiguity pairs remain excluded from
hints, not from shuffling. Automatic deferred sync points can limit the orders
Bevy explores. No upstream code is changed.

The same seed is applied to every candidate and schedule. For the same factory
and build it gives the same choices, independent of `SimSeed`. To explore other
orders, run checks with **different shuffle seeds**, not just more runs. Changing
system registration or the dependency graph can change the choices. The
reference retains the factory's executor; shuffled candidates use single-threaded
execution even if the factory selected a multithreaded executor.

All existing main-world schedules are configured before the first candidate
update, including `FixedUpdate` and startup schedules. A maintenance system in
Bevy's `Main` driver, ordered before `Main::run_main`, configures new or replaced
**not-yet-initialized** schedules at subsequent update boundaries without
resetting existing executors.
Normal lazy initialization is preserved: startup-dependent `Local::from_world`
values initialize at their normal execution time, and dormant schedules do not
initialize. Unchanged schedules are not rebuilt on every tick, preserving pending
deferred buffers. A private empty set records which schedule instances the
harness configured and requests their first build. Copying a schedule's public
shuffle seed does **not** copy this marker or establish its executor policy.

This variant requires the standard Bevy `Main` driver used by `Sim::new`; custom
app update drivers or replacing `Main` itself are not supported. Like `Sim`'s
executor policy, it cannot configure schedules created and immediately run
*within* a system. A newly discovered schedule that has already initialized
causes a clear panic **before** the harness changes its executor or settings.
Read-only validation also runs immediately after each candidate tick, before any
report is returned or `Sim` can reset a newly seen label's executor on the next
tick. Bevy's public APIs cannot preserve the live executor's pending deferred-buffer
bookkeeping when reconfiguring it. Make new/replacement schedules available at an
update boundary **before their first run**, not just before a later run. The same
restriction applies to schedules preinitialized by the factory. Sub-app schedules
are not accessible through `Sim` and are not shuffled. Systems added to an already
configured schedule inherit its shuffle setting on their normal rebuild. Do not
override the executor of a harness-configured schedule; overriding the shuffle
seed on a live schedule is rejected instead of forcing an unsafe rebuild. A factory that
makes ambiguities build errors still fails; use warning/ignore severity to explore
its ambiguities.

None of these variants exhaustively explores possible schedules. A pass is not
proof of race freedom. Order gameplay systems explicitly when behavior depends
on order, and configure immediately-run schedules according to the limitations
above.

## Input, bounds, and memory

`.script(InputScript::from_ron(text).unwrap())` replays the same recording on each
fresh simulation through `Sim::run_script`, one update at a time. The checker
stably sorts a playback copy once and uses a fresh event cursor per run, passing
only the current tick's events to `Sim::run_script`. It does not rescan the entire
recording on every tick; equal-tick action order and automatic tap releases are
preserved. The original recording remains unchanged in the report. Without a
script, the checker injects no input. Use the same seed, timestep, starting world,
plugins, and external state for repeatable replay.

`.ticks(n)` is required and must be positive. `.runs(n)` includes the reference
and must be at least two (default two). Missing/invalid budgets, unsupported
script versions, or an already-ticked factory result panic; factory and game
panics propagate. A tick budget bounds the number of updates, **not wall-clock
time**: a hanging system can hang the check. Use an external timeout if needed.

Runs execute sequentially. Run 1 completes its entire tick budget and its full
snapshot history stays in memory; the reference simulation is then dropped.
Candidates retain only their current snapshot and stop at their first mismatch.
The checker retains the earliest divergence found so far; later candidates only
advance through ticks that could beat it. Ties keep the lowest run number. A
mismatch at tick 1 ends the entire check, since no earlier compared tick exists.
Memory is **O(ticks × captured world size)** for the reference history, plus a
live candidate world, one candidate snapshot, and the retained divergence/diff.
No candidate snapshot histories are retained. Scripted checks additionally
retain an O(event count) sorted playback copy and a reusable current-tick input
buffer. There is no on-disk history or hash-only compression.

## Read a report

`DeterminismReport::Deterministic { runs, ticks }` means all requested snapshots
agreed. `DeterminismReport::Diverged(divergence)` holds the mismatch, replay
parameters, and best-effort hints. `println!("{report}")` prints a readable report;
`report.assert_deterministic()` panics with that report on failure.

A shortened example (IDs and type paths depend on your app):

```text
Nondeterminism detected: run 2 diverged from run 1 at tick 1 (of 4)
  seed: Some(42), reference seed: Some(42), variant: Repeat, runs: 2

~ 12v0 "Enemy 0"
    ~ hashmap_order::Enemy
        moves: 1 -> 0
```

- Runs are **one-based**, with run 1 as the reference. Tick 1 means one update
  has completed. Script event indices are **zero-based**: an event at script
  tick 0 is injected before the update compared as report tick 1. Report tick
  147 therefore follows update/script index 146.
- `~`, `+`, and `-` mean changed, added, and removed. Values run from the
  reference to the candidate. `12v0` is an entity index/generation, not a
  cross-run matcher. Resources appear as `resource <type>` without an entity.
- Nested field paths identify the serialized shape; JSON paths start at `$`,
  such as `$.moves` or `$.translation[0]`. Additions/removals and opaque changes
  may be reported at the whole-value level rather than inventing field data.
- This is the **earliest differing tick across all runs**. For example, a
  mismatch at tick 5 in run 3 beats tick 10 in run 2. If both differ at tick 5,
  run 2 wins the tie; later candidates need not execute tick 5 or beyond.

Hints are debugging leads, **not proof of causation**. Ambiguity hints read
existing schedule graph access conflicts for diverging types. For
`ShuffleAmbiguous`, `AmbiguityHint::order` records the chosen topological
`[before, after]` names from harness-configured executable schedules after the
diverging tick. A schedule carrying only a copied shuffle seed has no order hint,
since it may still use a multithreaded executor.
The readable report prints `shuffled order: before -> after`; the variant in the
replay parameters records the shuffle seed. These are configured orders, not
proof that either system ran (run conditions may skip them) or caused the bug.
Pairs unrelated to captured diverging types are omitted. Explicitly ordered
systems, unavailable schedules, or types without usable conflict data may yield
no hints. Hint collection itself never modifies schedules or execution order. With the optional
`track_location` feature, last-change hints include the recorded mutation site
when available; the last site is not necessarily the source of the bug:

```sh
cargo run -p titan_determinism --features track_location --example hashmap_order
```

`track_location` forwards to `bevy_ecs/track_location` and is off by default.

## JSON and reproduction

Reports implement `Serialize` and `Deserialize`, so tooling can use
`serde_json::to_string_pretty(&report)` and load a `DeterminismReport` again.
Divergence parameters include both construction-time seeds (if present), tick
and run budgets, variant, the full optional script, snapshot filters/clock policy,
and normalized float tolerance. `SnapshotSettings` and `FilterSettings` are
serializable copies of snapshot configuration, not a promise that the original
`SnapshotConfig`/`TypeFilter` types implement serde. Convert `SnapshotSettings`
back with `SnapshotConfig::from(settings)` when rebuilding a check.

The JSON is **not a standalone scenario or saved world**. Reproduction still
requires the same scenario factory, game code/build, initial world, timestep,
and relevant external state. A seed is only effective if gameplay uses it.
Successful reports contain just run/tick counts, not a replay payload.

The [`hashmap_order`](examples/hashmap_order.rs) example deliberately chooses
which enemy moves by taking the first `HashMap` entry. To reliably expose this
order-dependent bug without relying on random hash seeds, it uses a fixed
collision hasher and reverses insertion order between factories while keeping
entity allocation identical. This is a **controlled demonstration**, not a claim
that an unchanged factory with a normal `HashMap` fails on every check. The
example verifies that its two insertion variants actually yield different
orders on the current standard library, prints the divergence and JSON, and
exits successfully when that expected bug is detected. In real gameplay, use an
explicit stable priority/order (or sort keys) rather than treating hash iteration
as turn order.
