# titan_snapshot

A standalone library for observing an ECS world as a stable, readable JSON
document, then comparing two moments of the same simulation. It neither restores
worlds nor integrates with the test harness or MCP server.

## Quick start

```rust
use titan_snapshot::{DiffConfig, SnapshotConfig, WorldSnapshot};

// Register your reflected types with app.register_type::<T>() first.
let before = WorldSnapshot::capture(app.world(), &SnapshotConfig::default());
app.update();
let after = WorldSnapshot::capture(app.world(), &SnapshotConfig::default());

let diff = before.diff(&after, &DiffConfig { float_tolerance: 0.00001 });
println!("{diff}");
assert!(!diff.is_empty());
let machine_readable = serde_json::to_string_pretty(&diff)?;
let golden_file = serde_json::to_string_pretty(&after)?;
let loaded: WorldSnapshot = serde_json::from_str(&golden_file)?;
```

Run the headless example from the repository root:

```sh
cargo run -p titan_snapshot --example world_diff
```

Typical output includes entity lifecycle, component presence, nested fields, and
resources:

```text
~ 12v0 "Player"
    + world_diff::Grounded
      <absent> -> null
    ~ bevy_transform::components::transform::Transform
        translation[1]: 0.0 -> 2.309999942779541
~ resource world_diff::Score
    value: 0 -> 3
```

Exact IDs and float formatting depend on the app. Paths follow the serialized
shape: nested struct fields use `translation.y`, while Bevy/glam's custom serde
representation of a vector uses `translation[1]`. Machine-readable paths start
at `$`; punctuation in map keys is unambiguously quoted (`$["a.b"]`).

## Reflection and opaque values

For your own types, derive `Reflect` alongside `Component` or `Resource`, add
`#[reflect(Component)]` or `#[reflect(Resource)]`, and register them in the
world's `AppTypeRegistry`. Registration also registers reflected dependencies.
A bare `World` can initialize this registry and register types through its write
lock. Capture works without a registry, too: values are then opaque.

Each component/resource is keyed by its full reflected type path, falling back to
its ECS type name. This crate enables ECS `debug` names so non-reflectable types
remain identifiable in release builds. Values have explicit markers:

```json
{"kind": "reflected", "value": {"value": 11}}
{"kind": "opaque", "reason": "type is not registered for reflection"}
```

Opaque values are **not omitted**. Their additions, removals, and transitions to
or from reflected values are observable. Their internal mutations are not. An
empty diff means no *observable* changes, not proof of identical opaque state.
Serialization failures also become opaque, with a stable reason rather than
potentially iteration-order-dependent error text. Reflection-ignored fields are
not captured. Non-send resources are not exposed by the ECS resource iterator
and are not captured.

## Filters and defaults

`SnapshotConfig` has independent component and resource `TypeFilter`s. Each has
an optional allow list and a deny list of full type paths. `None` allows all;
`Some(empty_set)` allows none; deny always wins. `TypeFilter::only` builds an allow
list, and `filter.deny::<T>()` excludes a reflected type. Use explicit ECS type
names for non-reflected types.

The proposed defaults for this PR exclude only:

- `GlobalTransform`, derived from `Transform`;
- every `Time<*>` resource, which is usually per-frame clock noise;
- `AppTypeRegistry`, infrastructure which is itself non-reflectable.

Use `SnapshotConfig::all()` for an unfiltered capture. To include clocks with the
other defaults intact, set `exclude_time_resources = false`. Remove
`GlobalTransform` from `config.components.deny` to include it. Entities remain
present even if every component is filtered out, and `Name` metadata is retained
regardless of component filtering. Disabled entities are included. Internal
resource entities appear only as resources, not duplicated as game entities.

## Determinism and comparison

Entities sort numerically by index and generation, types and JSON map keys sort
lexicographically, and reflected sets sort by their canonical JSON elements.
Lists/arrays retain their order. Map sorting is explicit even if another crate
enables serde_json's `preserve_order`. Custom serde implementations must themselves
be deterministic; arbitrary user serializers that generate random data or encode
unordered sets as lists cannot be repaired automatically. Non-finite scalar
floats visited by the reflection serializer are written as `"NaN"`,
`"+Infinity"`, or `"-Infinity"`; custom serde implementations retain their own JSON
behavior (including conversion of non-finite floats to null).

Identity (`EntityId`) is separate from entity data, leaving room for future
matching strategies. The current matcher uses the full entity index/generation,
never `Name`; do not use it to match separate runs. Both empty and populated
entities are captured. Snapshots contain no timestamps or world-local pointers.

`DiffConfig::float_tolerance` is an absolute, inclusive tolerance. It applies
when at least one JSON number is floating-point; integer pairs compare exactly,
including large `u64`s. The default is zero. Negative or non-finite tolerances
act as zero. Added/removed subtrees are reported at their roots; changed
structures are recursively compared by object key or array index. Missing values
and explicit JSON null remain distinct through a diff's serde round-trip.

## Verification

```sh
cargo test -p titan_snapshot
cargo clippy -p titan_snapshot --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p titan_snapshot --no-deps
cargo run -p titan_snapshot --example world_diff
```

The integration tests cover repeated byte-identical capture, reordered hash maps
and sets, serde round-trips, entity generation reuse, nested diffs, filters,
opaque values, float tolerance, large integers, and common Bevy types/hierarchies.
