# titan_snapshot

A standalone library for observing an ECS world as a stable, readable JSON
document, then comparing moments of one simulation or logical entities across
runs. It neither restores
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
    ~ bevy_transform::components::transform::Transform
        translation[1]: 0.0 -> 2.309999942779541
    + world_diff::Grounded
      <absent> -> {}
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
remain identifiable in release builds. If multiple registered ECS descriptors
share a type path, their document keys gain a `[component_id:N]` suffix so no
value is overwritten. Suffixes are world-local; filters still match the original
path. They remain stable when one of the colliding values is removed (registered
descriptors persist). Registering a new colliding type between captures can change
a previously plain key into a suffixed key; register such types before the first
capture. Values have explicit markers:

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

`WorldSnapshot::diff` uses the complete entity index/generation, appropriate for
captures of **one run**. It does not infer identity from names. Both empty and
populated entities are captured. Snapshots contain no timestamps or world-local
pointers. For separate runs, use the explicit matcher below.

`DiffConfig::float_tolerance` is an absolute, inclusive tolerance. It applies
when at least one JSON number is floating-point; integer pairs compare exactly,
including large `u64`s. The default is zero. Negative or non-finite tolerances
act as zero. Added/removed subtrees are reported at their roots; changed
structures are recursively compared by object key or array index. Missing values
and explicit JSON null remain distinct through a diff's serde round-trip.

## Matching across runs

```rust
use titan_snapshot::{DiffConfig, EntityMatching};

let config = DiffConfig::default().with_entity_matching(EntityMatching::ByName);
let compared = saved_snapshot.diff_matched(&current_snapshot, &config);
assert!(compared.is_empty(), "{compared}");
let json = serde_json::to_string_pretty(&compared)?;

// Or use a captured, reflected game-owned stable ID, including struct keys:
let config = DiffConfig { float_tolerance: 0.00001 }
    .with_entity_matching(EntityMatching::ByComponent(StableId::type_path().into()));
```

`EntityMatchConfig` separates matching from the existing `DiffConfig` so existing
struct literals and ID-based callers remain source compatible. Its
`entity_matching` defaults to `ById` and its `diff` contains numeric tolerance.
`diff_matched` returns `MatchedWorldDiff`, containing the existing structural
`WorldDiff` under `diff`, plus identity `matches` and explicit `diagnostics`.

Choose an identity strategy deliberately:

- **`ById`**: same-world lifecycle comparisons, including generation reuse. This
  produces the same structural diff as `diff` and performs no reference rewriting.
- **`ByName`**: cross-run comparisons when every relevant entity has a unique,
  stable, case-sensitive `Name`. Name metadata remains available even when the
  component is filtered or there is no reflection registry. Empty names are valid
  keys, but must also be unique. Renaming is removal/addition, not a paired change.
- **`ByComponent(type_path)`**: game-owned stable identity. Register the type for
  reflection with `#[reflect(Component)]` and include it in the capture filter.
  The **entire serialized component value** is the key, not one selected field.
  Struct/object keys are recursively canonicalized; object insertion order does
  not matter, but array order does. Key equality is exact, never tolerant. Do not
  use world-local entity references or mutable gameplay state as identity. Use a
  unique full type path; world-local `[component_id:N]` disambiguation suffixes
  are not portable keys.

There is **no ID fallback**: absent names, missing/filtered key components, opaque
key components, and duplicate keys are reported explicitly. If a key is duplicated
on either side, **all** its occurrences on both sides are unmatched, including a
unique counterpart. Every unmatched earlier entity is removed and every unmatched
later entity is added, even if their raw IDs happen to coincide. Their captured
data is retained. `MatchedWorldDiff::is_empty()` includes diagnostics, so a golden
assertion cannot silently pass with uncomparable entities.

`matches` lists each unambiguous key with its `before` and `after` IDs, including
unchanged entities and keys present on only one side. JSON retains this complete
list. Text shows keys inline only for entities with structural changes, plus all
diagnostics, so unchanged matches do not obscure a golden-file failure:

```text
~ Name("Player") 3v1 -> 5v1
    ~ game::Health
        value: 100 -> 90
- Name("Enemy") 4v1 -> (none)
+ Name("Projectile") (none) -> 6v1
```

The `~`, `-`, and `+` markers mean changed, removed, and added, respectively, as in
`WorldDiff`. Component keys appear as `Component("game::StableId", {"number":7})`;
ID keys use `Id(3v1)`. Missing/opaque keys are labeled `unkeyed`, and duplicate keys
remain visible alongside their diagnostics. An empty matched diff prints only
`No observable differences.`

Structural `EntityDiff::entity` is the earlier ID for pairs/removals and the
later ID for additions. IDs can overlap between runs; use change kind and the
side-aware match/diagnostic metadata to resolve them. Names need not stay constant
when using a stable component key: a rename is then an ordinary changed entity.

### Entity references and limits

Capture identifies typed `Entity` values visited by reflection, including nested
fields, lists, `Children`, and references in resources. `ChildOf` is handled
explicitly because its built-in serde representation otherwise hides its parent.
These references use the reserved JSON shape `{"$titan_entity":"12v1"}` rather
than an untyped packed integer. In key-based diffs they become
`{"$titan_entity_key":<key>}` wherever the target has an unambiguous key. Logical
reference equality is exact even when numeric tolerance is enabled. Ordinary
numbers and ID-like strings are never treated as references.

Reflected sets containing references use `{"$titan_entity_set":[...]}` and maps
with reference-containing keys use `{"$titan_entity_map":[[key,value],...]}`.
After normalization, these sort first by the canonical JSON of their contained
`$titan_entity_key` and raw `$titan_entity` references, in traversal order (object
keys canonically sorted, array elements in order). The full canonical element or
entry value is only a tie-breaker. This keeps tolerated float changes from
reordering elements with distinct reference sequences; lists/arrays are **not**
re-sorted. Elements with identical references that differ only in tolerated floats
may still pair by full value and report a difference: this is not tolerance-aware
set matching. Ordinary maps and reference-free sets keep their original capture
representation.
Custom serializers must not emit these reserved `$titan_entity*` object shapes as
ordinary data.

References to absent, unkeyed, or ambiguous targets remain raw IDs: their logical
identity is unknown. Custom serde implementations on enclosing types bypass
reflected field traversal, so embedded entities in such blobs cannot be rewritten
(except the explicit `ChildOf` support). Older saved snapshots remain loadable,
but their untyped entity references cannot be normalized; regenerate golden files
when adopting key matching. Captures still are not restorable worlds. Filtering,
opaque values, list ordering (including `Children`), and nondeterministic custom
serializers retain the limitations described above.

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
Cross-run coverage includes reordered spawns, scalar/struct keys, canonical JSON
key order, missing/opaque/duplicate keys, lifecycle and renames, typed references,
entity-keyed maps/sets, reference-first collection ordering under float tolerance,
list order, exact reference identity under float tolerance, and matched-diff
text/JSON round-trips.
