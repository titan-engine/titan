# titan_diagnostics

Structured, local JSON reports for errors and panics in running or headless
Titan/Bevy apps. No upstream ECS patches and no change to the app's error policy:
reports are written **before** calling the previous `FallbackErrorHandler` or
panic hook. Explicit per-command/observer error handlers still take precedence.

## Headless usage

Install the tracing layer once, then add the plugin after your other plugins and
configured error handler. The plugin adds `FrameCountPlugin` if absent.

```rust,no_run
use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use titan_diagnostics::{DiagnosticsLayer, DiagnosticsPlugin};
use tracing_subscriber::prelude::*;

tracing::subscriber::set_global_default(
    tracing_subscriber::registry().with(DiagnosticsLayer),
).expect("install the app's subscriber once");

fn load_level() -> Result {
    tracing::warn!(asset = "levels/demo.ron", "level file is missing");
    Err(BevyError::error("cannot load levels/demo.ron"))
}

let mut app = App::new();
app.add_plugins(DiagnosticsPlugin {
    directory: "reports/my-game".into(),
    app_name: "my-game".into(),
    app_version: Some("0.1.0".into()),
    max_reports: 32,
    max_recent_logs: 64,
});
app.add_systems(Update, load_level);
app.update();
```

For an existing `bevy_log::LogPlugin`, set its `custom_layer` to
`|_| Some(Box::new(titan_diagnostics::DiagnosticsLayer))` instead of installing
another subscriber. With `DefaultPlugins`, configure the `LogPlugin` through
`PluginGroup::set`, then add `DiagnosticsPlugin` afterward. This crate does not
replace the existing subscriber or filters, and does not require a renderer.
The layer is compatible with the public `bevy_log::BoxedLayer` type.

Filters must allow Bevy's INFO-level schedule/system spans and WARN/ERROR
events (for example, the default INFO filter). Filtering out INFO spans loses
schedule/panic-system context. Without the layer, fallback error reports still
have the precise `ErrorContext` name, but `schedule` is null and recent logs are
empty. Events logged through `log` macros require the usual `LogTracer` bridge
(provided by Bevy's `LogPlugin`); the layer directly captures `tracing` events.

## Context and process-global ownership

`ErrorHandler` is a plain function pointer with no world argument. The plugin
therefore has one process-global sink, owned by the app's `DiagnosticsState`
resource. **Only one live diagnostics-enabled App is supported per process.**
Adding a second is rejected explicitly. Dropping the app releases the sink and
allows a later app to claim it; parallel tests should use child processes or
serialize the diagnostics apps. Do not copy the installed fallback handler to
other worlds, remove the ownership resource, or overwrite the fallback handler
while diagnostics is active. The installed panic hook remains in place but is
inert when there is no active app. Install any other panic hooks first, or chain
them rather than replacing this hook afterward.

Thread-local schedule stacks track Bevy's public tracing spans, including
nested schedules. Worker threads with detached system spans and recognized
ECS failure context use the active caller schedule only when all active
schedules belong to a single caller thread. An unrelated thread's panic with
no ECS context does not inherit the active app schedule. If concurrent schedule execution makes that fallback ambiguous, the
schedule is null, never an unrelated thread's innermost label. Running
independent worlds concurrently is still **not supported for app ownership**:
the public fallback API cannot identify which world called it. Panics outside
that app can also be recorded while it is alive; they have null context when
there is no system span. No entity, asset, or scene identity is invented: include those details in
the error message or structured warning fields.

`frame` is the app's `FrameCount` sampled before `Main::run_main`, so startup
errors have frame 0 and systems in a normal update share that update's frame
(including `Last`, before its increment). It is not the ECS `last_run` change
tick. For manually driven schedules, call `refresh_frame(app.world_mut())`
before running them. Before any refresh, or if `FrameCount` is absent, the
frame is null. Mutating `FrameCount` during an update is not tracked.

A panic hook runs **before** Bevy catches a panic and produces `ErrorContext`.
It records the original payload, location, backtrace and active system span
when available. Bevy may subsequently invoke the fallback handler with a
contextual `System panicked`/command/observer error; that produces a separate
`error` report. Returned errors whose previous handler panics can likewise
produce both an `error` and `panic` report. This preserves both evidence and
existing behavior, rather than swallowing or resuming panics differently.
A Rust panic does not necessarily exit (the caller may catch it); signals,
`process::abort`, OOM aborts, and `process::exit` are not Rust panics and are not
covered. IO is synchronous, including in the hook; no asynchronous shutdown is
required. This is best-effort reporting, not a hardened crash-handler.

## Files, retention and failure behavior

Each report is written to a temporary file in the output directory, synced,
then atomically replaced via `tempfile::NamedTempFile::persist`. Readers never
see partially serialized JSON. The directory is created lazily. Atomic replace
is not a guarantee against power-loss of the directory entry.

Filenames are predictable from the report ID:
`titan-diagnostics-<32-hex-session-time>-<8-hex-pid>-<16-hex-sequence>.json`.
Session time is Unix nanoseconds. Reports contain no user-controlled path
segments. Deduplication replaces the **same file**, increments `count`, and
updates latest frame, context, logs, message and reporting backtrace, keeping
the first frame/time. The key is kind, severity, context kind/name/associated
system/on-set, schedule, message (excluding Bevy's appended origin backtrace),
and panic location. Frame, change tick, logs and backtraces are not part of the
key. Panic payloads are compared verbatim. Bevy does not expose a separate
message/backtrace accessor, so exclusion of its appended origin stack is
format-based, requiring a trailing stack and Bevy error-construction symbols
or source locations. Full stacks use their last recognized capture boundary;
filtered stacks stop at a verified construction frame and may retain a stable
capture/construction prefix in the key. Embedded ordinary Rust backtraces stay
part of the message key. Unrecognized formats remain part of the key;
messages deliberately imitating Bevy construction stacks can alias. Deduplication covers the most recent `max_reports` distinct reports in
this app session, not reports loaded from previous runs.

After each successful write, retention limits all regular report files matching
that exact filename grammar to `max_reports`, including earlier sessions,
oldest modification time first. Other names and symlinks are not pruned. Use a
private, trusted directory per game/process; do not share it with concurrent
writers. The directory and matching filenames are reserved for reports.
`max_reports` must be nonzero; `max_recent_logs = 0` disables the log buffer.
Both the report/dedupe cache and warning-event buffer are bounded by entry
count, **not byte size**; very large errors or log fields can still be large.

IO/serialization/retention failures never prevent the previous handler from
running. Panics from a broken error Display formatter are caught before
forwarding the original error, and exposed through the same status (Rust still
invokes the previous panic hook for that caught formatter panic). They are exposed by `DiagnosticsState::last_write_error()` and are
not recursively logged. A later successful write clears the error. An
unwritable directory can therefore yield no report; callers can inspect that
status after recoverable errors. Reports may contain sensitive data from
messages, paths and stack traces; local storage is not redacted or encrypted.

## JSON schema (version 1)

All fields below are present. Optional values are JSON null. Consumers should
check `schema_version` and tolerate additional fields in future versions.

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Currently `1` |
| `id` | string | Unique session/process/sequence ID, used in the filename |
| `kind` | string | `error` (fallback handler) or `panic` (hook) |
| `severity` | string or null | Bevy's advisory severity (`Ignore`, `Trace`, `Debug`, `Info`, `Warning`, `Error`, `Panic`) |
| `app_name`, `app_version` | string, string or null | Configured application identity |
| `context` | object or null | Fields described below |
| `schedule` | string or null | Innermost active schedule label, formatted with Debug |
| `frame`, `first_frame` | integer or null | Latest/first frame snapshots (`u32`, wraps with Bevy) |
| `message` | string | Error Display or original panic payload (placeholder for non-string panics) |
| `backtrace` | string or null | Reporting-point `std::backtrace` capture; enable `RUST_BACKTRACE=1` |
| `location` | string or null | Panic source file/line/column |
| `count` | integer | Matching occurrences, saturating `u64` |
| `first_seen_unix_ms`, `last_seen_unix_ms` | integer | First/latest wall-clock Unix milliseconds |
| `recent_logs` | array | Latest occurrence's bounded WARN/ERROR events, oldest first |

`context` fields: `kind` (`system`, `command`, `observer`, `run_condition`),
`name` (string), `last_run` (ECS change tick or null), `system` (associated
run-condition system name or null), `on_set` (run-condition bool or null).
For hook reports, kind/name come from an active system/command tracing span,
not a guessed `ErrorContext`; unavailable fields stay null.

Each recent log has `unix_ms` (integer), `level` (`WARN`/`ERROR`), `target`
(string), and `fields` (object mapping field names to formatted strings,
including `message`). String fields retain their values; other values use
tracing's Debug visitor representation. A panicking or failed field formatter
is contained by this layer and retained as
`<diagnostics: field formatter failed>` instead of unwinding into the app.
Rust still invokes the previously installed panic hook for caught formatter
panics; this layer does not create extra app panic reports for them. Other
subscriber layers are responsible for their own formatting failures.

The `backtrace` field is captured at the reporter, not at error construction.
If another workspace crate enables `bevy_ecs/backtrace`, Bevy's error Display
also includes its origin backtrace in `message`; it is preserved verbatim.
The schema does not scrape stderr or extract entity IDs from prose.

Example (backtrace disabled, second occurrence):

```json
{
  "schema_version": 1,
  "id": "000000000000000018ba32cf46a00000-00001234-0000000000000001",
  "kind": "error",
  "severity": "Error",
  "app_name": "my-game",
  "app_version": "0.1.0",
  "context": {
    "kind": "system",
    "name": "my_game::load_level",
    "last_run": 12,
    "system": null,
    "on_set": null
  },
  "schedule": "Update",
  "frame": 1,
  "first_frame": 0,
  "message": "cannot load levels/demo.ron\n",
  "backtrace": null,
  "location": null,
  "count": 2,
  "first_seen_unix_ms": 1780000000000,
  "last_seen_unix_ms": 1780000000016,
  "recent_logs": [
    {
      "unix_ms": 1780000000016,
      "level": "WARN",
      "target": "my_game",
      "fields": {
        "asset": "levels/demo.ron",
        "message": "level file is missing"
      }
    }
  ]
}
```

## Follow-ups

BRP exposure and integrations with `titan_test`, `titan_fuzz`,
`titan_determinism` and `titan_mcp` are deliberately not implemented here.
Automatic attribution across multiple simultaneous apps/sub-apps would need a
world-aware public upstream hook; this crate does not patch `bevy_ecs` to add one.
