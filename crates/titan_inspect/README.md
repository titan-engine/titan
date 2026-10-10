# `titan_inspect`

Read-only, bounded schedule and asset inspection for agents using the Bevy Remote Protocol
(BRP). This is separate from `titan_remote`: that crate controls time and captures
screenshots; this crate describes schedules and asset diagnostics. Neither
requires the other. Asset inspection uses small read-only additions to Bevy's
asset enumeration and handle reflection metadata; it never reflects asset contents.

```rust,no_run
use bevy_app::App;
use bevy_remote::RemotePlugin;
use titan_inspect::InspectPlugin;

let mut app = App::new();
app.add_plugins((RemotePlugin::default(), InspectPlugin));
app.finish();
app.cleanup();
app.update();
```

Add Bevy's `RemoteHttpPlugin` (enable `bevy_remote/http` in your app) to serve BRP
via HTTP. Inspection itself does not require HTTP, rendering, or a GPU. BRP has
no authentication by default: bind to loopback or secure the transport yourself.

## Methods

All methods accept an optional `limit` (default **64**, integer **1..=256**).
Unknown fields and invalid values return JSON-RPC `INVALID_PARAMS` (`-32602`).
Lists use `{ "items": [...], "total": N, "truncated": bool }`. The same limit
applies independently to the outer list and each nested list. System details are
computed only for the selected outer page, using a compact declaration graph
rather than materializing its transitive closure. Ambiguity paging selects a
bounded prefix of borrowed pair-name keys before building conflict details,
without allocating a JSON object for every conflicting pair. Thus a system with
many sets, conditions, edges, or conflicts cannot silently overflow the limit.
`total` counts the full list before truncation. No pagination is provided.

### `titan.schedules`

```json
{"jsonrpc":"2.0","id":1,"method":"titan.schedules","params":{"limit":64}}
```

Example result (the app's other schedules are omitted here):

```json
{"items":[{"name":"Update","status":"initialized","system_count":3,"executor_kind":null,"executor_kind_available":false}],"total":1,"truncated":false}
```

Schedule names are the label's Debug representation, matching Bevy's BRP
schedule naming. `system_count` includes executor-inserted systems such as
`ApplyDeferred`. Status is `initialized`, `uninitialized`, `pending_rebuild`, or
`running`. Running schedules are temporarily removed from `Schedules` by Bevy;
we still list their names, but their system counts are `null`.

**Executor kind is unavailable:** this Bevy revision supports custom executors
but provides no public getter. `null`/`false` explicitly mean unknown, not an
assumed default. A future upstream getter can fill this in without changing the
response shape.

### `titan.systems`

```json
{"jsonrpc":"2.0","id":2,"method":"titan.systems","params":{"schedule":"Update","limit":64}}
```

Example result:

```json
{"schedule":"Update","status":"initialized","ordering":"declared_transitive","systems":{"items":[{"name":"game::move_player","exclusive":false,"sets":{"items":["Gameplay"],"total":1,"truncated":false},"run_conditions":{"items":["game::playing"],"total":1,"truncated":false},"before":{"items":["game::render_state"],"total":1,"truncated":false},"after":{"items":[],"total":0,"truncated":false}}],"total":1,"truncated":false}}
```

System names are full Bevy system names. Sets include direct and inherited sets
(including Bevy's automatic function-type and anonymous sets). Conditions include
both system conditions and inherited set conditions, sorted and deduplicated by
name; they are descriptions, not evaluated boolean results. Exclusive systems
are identified from their initialized access.

`before`/`after` describe **declared, transitive** dependencies expanded through
sets, by system name. This is not execution order: unrelated systems can run in
parallel. Weak dependencies are declarations too, even if Bevy drops them during
building. Edges inserted by build passes are not included. In particular, this
must not be used to infer synchronization points or a guaranteed runtime order
for weak dependencies. Exact executable dependency introspection is a proposed
upstream follow-up.

### `titan.ambiguities`

```json
{"jsonrpc":"2.0","id":3,"method":"titan.ambiguities","params":{"schedule":"Update","limit":64}}
```

Example result:

```json
{"schedule":"Update","status":"initialized","ambiguities":{"items":[{"systems":["game::move_player","game::reset_player"],"conflicts":{"items":["game::Position"],"total":1,"truncated":false},"world_access":false,"world_wide":false}],"total":1,"truncated":false}}
```

Reports Bevy's built ambiguity analysis, including conflicting component/resource
type names (resources are components in this ECS revision). `world_wide: true`
explicitly identifies a world-wide/non-specific access conflict: the empty type
list means **all types**, not no conflict. This includes exclusive World-access
systems and non-exclusive systems with unrestricted component access, such as
`Query<EntityMut>`. Exclusivity is reported separately by `titan.systems`;
`world_wide` does not imply it. `world_access` is retained as an equivalent alias
for compatibility. Both flags are false for conflicts on named types.

For example, two unordered `Query<EntityMut>` systems produce a pair like:

```json
{"systems":["game::edit_a","game::edit_b"],"conflicts":{"items":[],"total":0,"truncated":false},"world_access":true,"world_wide":true}
```

Explicitly ignored ambiguities are omitted, just as they are by Bevy. Detection works even
with the default `ambiguity_detection: Ignore` logging setting.

### `titan.assets`

Requires `AssetPlugin` and types registered with Bevy's usual `init_asset::<T>()`
(already done by built-in asset plugins). Neither asset `Reflect` nor
`register_asset_reflect` is required. Without an asset plugin the list is empty.

```json
{"jsonrpc":"2.0","id":4,"method":"titan.assets","params":{"state":"failed","path_prefix":"textures/","limit":64}}
```

Example result (IDs are illustrative, session-local opaque strings):

```json
{"items":[{"id":"game::Texture:index:7","path":"textures/missing.png","type":"game::Texture","state":"failed","error":"Path not found: textures/missing.png","server_managed":true,"dependency_state":"failed","dependency_error":"Path not found: textures/missing.png","recursive_dependency_state":"failed","recursive_dependency_error":"Path not found: textures/missing.png","dependencies":null,"dependency_chain":{"items":[],"total":0,"truncated":false},"dependency_chain_complete":false}],"total":1,"truncated":false}
```

Optional filters are combined with AND, applied **before** computing `total`
and truncating:

- `type`: exact full asset type path (as returned in `type`).
- `state`: `not_loaded`, `loading`, `loaded`, or `failed`, for the asset
  **itself**. A loaded scene with a failed texture still has `state: "loaded"`;
  inspect its dependency states to diagnose it.
- `path_prefix`: case-sensitive string prefix of the full Bevy asset path,
  including source and label where present. Pathless assets never match a prefix,
  even the empty prefix. Omit this filter to include them.

Assets are sorted by path (null first), then type, then session-local ID. IDs
identify records and dependency links within the current app, not across runs.
The list combines server-tracked assets (including loading/failed ones) and
stored `Assets<T>` values, deduplicated by ID. No strong handles are retained.
Assets created in code have `path: null`. Stored assets outside the server have
`state: "loaded"`, `server_managed: false`, and null server dependency states:
this means present in storage, not a fabricated server load result. An unknown
referenced asset has null state; an unavailable type name is null, not guessed.

`dependencies` lists declared **direct** dependencies with ID, path, type,
state, and error. `dependency_chain` is a breadth-first list of edges across
reachable dependencies. Each edge has `parent_id` and the dependency's summary.
Sibling edges use the same sort order as assets; duplicate IDs are deduplicated
per parent and cycles are visited once. The request limit independently bounds
both lists, and each reports its full `total` and `truncated`.

For example, a scene whose parent asset references a missing texture reports:

```json
{"id":"game::Scene:index:1","path":"scene.demo","state":"loaded","dependency_state":"loaded","recursive_dependency_state":"failed","recursive_dependency_error":"Path not found: missing.demo","dependency_chain":{"items":[{"parent_id":"game::Scene:index:1","id":"game::Scene:index:2","path":"parent.demo","type":"game::Scene","state":"loaded","error":null},{"parent_id":"game::Scene:index:2","id":"game::Scene:index:3","path":"missing.demo","type":"game::Scene","state":"failed","error":"Path not found: missing.demo"}],"total":2,"truncated":false},"dependency_chain_complete":false}
```

This excerpt omits the direct list and other fields. Dependency graphs come from
`VisitAssetDependencies` on **stored** values. If an asset is not yet stored
(loading/failed), its direct list is `null`, not an invented empty list. If any
reachable node cannot be visited, `dependency_chain_complete` is false. This
flag describes graph availability, independently of page truncation. Failed
leaf assets thus make it false even when the useful chain to that failure is
visible. Embedded dependencies loaded inside a loader and not declared on the
asset value cannot be reconstructed; server dependency errors are still reported
where available. No dependency edges are inferred merely from matching errors.

### `titan.asset_failures`

```json
{"jsonrpc":"2.0","id":5,"method":"titan.asset_failures","params":{"path_prefix":"textures/","limit":64}}
```

Example result:

```json
{"items":[{"sequence":1,"id":"game::Texture:index:7","path":"textures/missing.png","type":"game::Texture","state":"failed","error":"Path not found: textures/missing.png"}],"total":1,"truncated":false,"capacity":256,"dropped":0,"history_truncated":false}
```

Captures every `UntypedAssetLoadFailedEvent` in `Last`, before BRP dispatch,
independently of whether anyone polls. Install `InspectPlugin` before startup
loads to retain startup failures. The fixed **256-record** FIFO ring survives
message expiry, dropping handles, and later successful reloads. Repeated failures
of the same asset are separate records. Results are newest-first; `sequence`
is a monotonically increasing capture number, not a timestamp. Optional `type`
and `path_prefix` filters work as above; a `state` parameter is not accepted
because every record is a failure. `total` counts matching **retained** records.
`dropped` counts all evicted records (before filtering), and
`history_truncated` explicitly reports retention loss, distinct from response
page truncation. These are historical errors, not claims about current state.
Failures of pathless `AssetServer::add_async` assets retain `path: null` and
never match `path_prefix`, including an empty prefix.

Both asset methods are read-only: they never load, reload, or retain assets.
A BRP handler does not advance tasks, alter asset storage, or drain messages.

Run `cargo run -p titan_inspect --example assets` for a headless in-memory
asset source that loads a valid asset, a missing one, and a scene with a failed
transitive dependency, then prints both responses through BRP's request mailbox.
The shared fixture is tested over that same BRP dispatcher, including failure
retention for 300 frames, filtering, nested truncation, ring eviction, pathless
and UUID assets, cycles, and a loading-state/read-only check.

## Availability and stability

These methods never initialize, run, rebuild, or reconfigure schedules, and never
evaluate run conditions. For an uninitialized schedule or one awaiting rebuild,
`systems`/`ambiguities` is `null` with the corresponding status, not an empty list
or stale build. Unknown, currently running, or non-unique Debug label names return
`INVALID_PARAMS`. Running `Main`/`RemoteLast` are normally unavailable during BRP
processing; their names remain visible in `titan.schedules`.

Bevy moves conditions into private executable storage on build. `InspectPlugin`
installs a **non-mutating build pass** at plugin finish to retain their names.
Valid captures made by explicitly observing and initializing schedules before
finish (including in earlier plugin finish hooks) are preserved, along with
their original capture pass, rather than discarded or made to await a rebuild.
Only schedules present when this plugin's finish hook runs are automatically
observed. A later plugin's finish hook may create or replace schedules; plugin
order independence applies to BRP registration, not condition-capture timing.
For schedules added/replaced later (including in later finish hooks), call
`titan_inspect::schedules::observe_schedule(&mut schedule)` before their first
build, **after all build passes that modify conditions**. Calling it again moves
capture to the end of the pass list. Condition-modifying passes installed after
capture are not supported; Bevy has no post-build public condition getter. If a
later pass only inserts systems, valid captures for existing systems are retained;
late-added systems (or inherited sets absent from the snapshot) report their own
`run_conditions: null`, not an invented empty list. Install capture after
system-inserting passes too when complete coverage of their additions is needed.
Attaching after a build cannot recover names until the next rebuild;
`run_conditions: null` explicitly reports unavailable capture. To reject stale
capture from replacement schedules, validation uses a live pass token and private
boxed-system allocation identities (never transmitted). Schedules containing
only zero-sized systems, such as `ApplyDeferred`, have no unique allocation
witness and conservatively report conditions as unavailable. The pass captures
again on every rebuild and does not change scheduling semantics.

Lists are sorted lexically by names (ambiguity pairs are canonicalized); nested
name lists are deduplicated. Duplicate system names remain separate records,
with private declaration-instance order breaking sorting ties. Names are descriptive, not
unique instance identifiers: edges cannot distinguish two instances with the
same name. No internal node IDs are sent. Stability depends on stable Debug and
system names supplied by the app; labels containing pointers or changing values
cannot be made stable by this crate.

Run `cargo run -p titan_inspect --example schedules` for a headless app with
ordered/unordered systems and a set condition, printing all three BRP responses.

Tests exercise all three methods through BRP's actual request mailbox, including
repeated calls, fresh app launches, ordering, conditions, conflicts, and limits.
The `schedules` and `assets` modules own their inspection areas independently.
