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

## Failures and revisions

Successful step, command, and input requests advance the inspector's revision.
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
