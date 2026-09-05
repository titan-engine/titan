# In-process inspection

`Inspector` executes `titan_protocol::RequestEnvelope` values while the caller
holds exclusive access to an `App`. Call it between schedules. A transport
queues requests; it must not read or mutate the world on its own thread.

The built-in requests expose capabilities, runtime status, named entities and
component type names, and exact fixed-tick stepping. Component values remain
opaque until a game or reflection adapter exposes them.

## Game adapters

Register commands with `Inspector::register_command`, supplying
`CommandMetadata` and a handler whose argument type implements
`serde::de::DeserializeOwned`. Arguments arrive as a JSON object. Use
`#[serde(deny_unknown_fields)]` to reject misspelled argument names. Commands
are listed in name order; registering a duplicate or blank name is an error.
Validate arguments before changing the world. Registering a command explicitly
exposes that game operation; the field-mutation flag controls `SetField`, not
registered game commands.

Register an input hook with `register_input_handler`. It validates the game's
logical action names and values and queues them for a future frame. Injection
requires a controlled runtime. Frame 1 is the first fixed tick; a request for
frame N must arrive before N has completed. The hook decides whether duplicate
frame submissions replace or reject existing input, and should document that
choice. Deferred writes are flushed before and after each input hook, including
a rejected hook; these writes are not transactional. An input response's `applied_frame` identifies the scheduled frame;
`observed_frame` is the runtime's current completed tick.

Register a capture hook with `register_capture_handler`. It receives shared
application access and returns image dimensions, format, artifact location,
and checksum. Capture does not advance the clock or state revision. Render
from current world state so changes since the last fixed tick are visible.

Capabilities advertise registered adapters. Unsupported adapters and unknown
command names return structured protocol errors.

## Controlled step budgets

Each `Step` request is limited to 10,000 frames and, on native targets, five
seconds of cooperative execution time. Configure an inspector independently of
transport and CLI limits:

```rust
use std::time::Duration;
use titan::inspection::{InspectionConfig, Inspector, StepBudget};

let mut inspector = Inspector::new(InspectionConfig::controlled("game", "project"));
inspector.set_step_budget(StepBudget {
    max_frames: 1_000,
    max_duration: Some(Duration::from_secs(2)),
});
```

Requests above `max_frames` return `invalid_value` with `requested_frames` and
`max_frames` details before startup, deferred writes, or fixed ticks run. A zero
frame request still performs normal checked startup when time permits.

Native time checks run before execution, after startup, and between completed
ticks, including after the final tick. A timeout returns `timeout` with requested
and completed frame counts plus elapsed and allowed microseconds. Completed work
remains visible in `observed_frame`; the successful-operation revision does not
advance. Application failures retain their existing structured errors. Timeouts
cannot interrupt an individual system, startup schedule, or tick; hosts needing
hard interruption must isolate execution in a bounded process.

Set `max_duration: None` to disable the native time limit; zero duration rejects
execution immediately. WebAssembly enforces the frame cap but does not use the
native clock limit; browser hosts can additionally enforce their own clock policy.
A CLI transport timeout stops waiting for a response and does not cancel the
runtime request. Inspect state before retrying a timed-out mutation.

## Failures and revisions

Successful field edit, step, command, and input requests advance the inspector's revision.
Read requests and rejected requests leave it unchanged. A revision tracks
successful inspection operations; it is not a world checksum or transaction ID.

`App::try_advance_fixed` stops at the first deferred-command failure and reports
it. A failed fixed update still counts as an executed tick. Startup failures
stop before the first tick. `App::apply_deferred` provides an explicit structural
safe point for command handlers. Structured inspection failures include the
failed operations and their entity IDs.

Neither game handlers nor deferred command batches are transactional. Earlier
successful world changes are not rolled back on failure. Always inspect the
response's `observed_frame` and re-inspect state after a partially executed
request. Handlers should perform validation before mutation.

## Procedural RPG acceptance

The RPG example exercises command discovery, named entity inspection, queued
logical input, exact stepping, shrine activation, and a software capture through
protocol requests. Run its acceptance tests with:

```sh
cargo test --example procedural_rpg
```

The original direct replay remains an independent exact-image reference.

## Explicit component fields

The RPG's `Position` uses the opt-in `Inspect` derive to register its two tile
coordinates. This minimal slice supports named, nongeneric component structs
and explicitly annotated `i32` fields:

```rust
use titan::{Component, Inspect, inspection::{InspectionConfig, Inspector}};

#[derive(Component, Inspect)]
struct Position {
    /// Map tile coordinate
    #[inspect(writable, minimum = 0, maximum = 19, unit = "tile")]
    x: i32,
    /// Map tile coordinate
    #[inspect(writable, minimum = 0, maximum = 13, unit = "tile")]
    y: i32,
}

let mut inspector = Inspector::new(InspectionConfig::controlled("game", "project"));
inspector.register_inspectable::<Position>()?;
# Ok::<(), titan::inspection::ProtocolError>(())
```

Field names and `i32` type labels are generated; field Rust doc comments become
descriptions. `minimum`, `maximum`, and `unit` are optional enrichments. Bounds
accept Rust expressions convertible through `f64::from`, including map constants.
An annotated field is read-only unless it includes `writable`; unannotated fields
are not registered and need no serialization support. The derive does not expose
anything until explicitly registered with an inspector, and registration does
not enable mutation. `Component` itself still permits opaque types.

Generated registration uses the same typed decoding, numeric bounds, permissions,
and getters/setters as the manual API below. Use manual registration for other
field types, computed values, or validators enforcing additional game invariants.
This slice does not generate type-level documentation, nested reflection, or
component serialization. Registration is sequential; an error leaves any earlier
successful field registrations in place, just as repeated manual calls do.

Games expose individual fields with `Inspector::register_field::<Component, Value>`.
This opt-in API does not require component serialization or derived reflection:

```rust
# use titan::{Component, inspection::{InspectionConfig, Inspector}};
# use titan_protocol::FieldMetadata;
# #[derive(Component)] struct Position { x: i32 }
# let mut inspector = Inspector::new(InspectionConfig::controlled("game", "project"));
inspector.register_field::<Position, i32>(
    "x",
    FieldMetadata {
        type_name: "i32".into(), description: "Horizontal tile".into(),
        writable: true, minimum: Some(0.0), maximum: Some(19.0),
        unit: Some("tile".into()),
    },
    |position| position.x,
    |_position, _value| Ok(()),
    |position, value| position.x = value,
)?;
# Ok::<(), titan_protocol::ProtocolError>(())
```

The component key is its full Rust type name, matching entity inspection.
`EntityDetails.components[component]` contains an object of registered field
values; unregistered component types remain `null`. The optional
`component_fields[component][field]` object describes each exposed field's type,
bounds, unit, and writability. `component_field_metadata()` also lists registered
fields for components with no current instances, so diagnostic API summaries
remain useful after entities are removed.

`register_read_only_field::<Component, Value>(name, metadata, getter)` exposes a
value without a setter. Registration normalizes `metadata.writable` to match the
API used. `type_name` is a caller-supplied display label; the generic `Value`
type controls JSON decoding. Duplicate or blank field names and nonfinite or reversed numeric bounds
are rejected without replacing an existing registration.

`SetField` first checks `InspectionConfig.mutation_enabled`, which is false by
default. It then validates the full entity generation and component presence,
finds the writable field, deserializes its typed value, checks declared numeric
bounds, and calls the immutable validator. Only then does the infallible setter
receive mutable component access. Use the validator for additional game rules
and the setter only to assign the validated field. Rejected edits leave the
component, fixed tick, and revision unchanged; successful edits increment the
revision once and return `applied_frame` without advancing the clock. Field edits
do not flush deferred commands or run schedules.

Disabled mutation returns `mutation_disabled` before examining the target.
Missing entities or components return `not_found`; unregistered or read-only
fields return `read_only`; invalid types, bounds, or game validation return
structured errors. `Mutate` is advertised only when mutation is enabled and at
least one writable field is registered. Getter serialization failures return
`internal`. Field getter/setter callbacks should not panic.

## Read-only game queries and live hosts

`Inspector::register_query<A>` exposes explicitly registered game-owned state
through shared `&App` access. `QueryMetadata` describes the name and typed JSON
arguments, like command metadata. `Queries` lists available reads; `Query`
returns `QueryResult { value }`. Reads do not advance time, drain deferred writes
or increment the revision. Query callbacks must return bounded data. Use
`#[serde(deny_unknown_fields)]` on arguments when extra fields should be rejected.
This is an additive protocol capability, advertised as `Operation::Query`.

Live hosts retain ownership of their actual application. `handle_with_policy`
and `handle_json_with_policy` borrow that app and inspector and enforce control
opt-in; read-only sessions allow queries and captures. Existing `BrowserSession`
remains an owning wrapper for isolated paused instances. DOM origin checks and
native authentication remain transport responsibilities.

A player calls `set_controlled` at the pause/resume safe point so status and
step/input capabilities match clock ownership. `note_external_change` accounts
for local ticks or changes such as restart that did not pass through a request.
The host must not use a transport timeout as cancellation or let a transport
worker access the world. Frame identifies completed simulation time; revision
also distinguishes local changes at the same frame.
