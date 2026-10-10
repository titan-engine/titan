# `titan_inspect`

Read-only, bounded schedule inspection for agents using the Bevy Remote Protocol
(BRP). This is separate from `titan_remote`: that crate controls time and captures
screenshots; this crate describes the code that changes the world. Neither
requires the other. No upstream Bevy code is modified.

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
{"schedule":"Update","status":"initialized","ambiguities":{"items":[{"systems":["game::move_player","game::reset_player"],"conflicts":{"items":["game::Position"],"total":1,"truncated":false},"world_access":false}],"total":1,"truncated":false}}
```

Reports Bevy's built ambiguity analysis, including conflicting component/resource
type names (resources are components in this ECS revision). An empty type list
with `world_access: true` denotes an exclusive World-access conflict. Explicitly
ignored ambiguities are omitted, just as they are by Bevy. Detection works even
with the default `ambiguity_detection: Ignore` logging setting.

## Availability and stability

These methods never initialize, run, rebuild, or reconfigure schedules, and never
evaluate run conditions. For an uninitialized schedule or one awaiting rebuild,
`systems`/`ambiguities` is `null` with the corresponding status, not an empty list
or stale build. Unknown, currently running, or non-unique Debug label names return
`INVALID_PARAMS`. Running `Main`/`RemoteLast` are normally unavailable during BRP
processing; their names remain visible in `titan.schedules`.

Bevy moves conditions into private executable storage on build. `InspectPlugin`
installs a **non-mutating build pass** at plugin finish to retain their names.
Only schedules present when this plugin's finish hook runs are automatically
observed. A later plugin's finish hook may create or replace schedules; plugin
order independence applies to BRP registration, not condition-capture timing.
For schedules added/replaced later (including in later finish hooks), call
`titan_inspect::schedules::observe_schedule(&mut schedule)` before their first
build, **after all build passes that modify conditions**. Calling it again moves
capture to the end of the pass list. Condition-modifying passes installed after
capture are not supported; Bevy has no post-build public condition getter. Attaching after a build cannot recover names until the next rebuild;
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
The `schedules` module owns this area so asset inspection (#76) can be added as
an independent module and method registration.
