# titan_mcp

`titan_mcp` is a small, synchronous MCP stdio server that lets an AI agent inspect
and control a running Titan game through the Bevy Remote Protocol (BRP).
It is a separate process: a game crash does not disconnect MCP, and MCP/HTTP
client dependencies do not enter your game binary. No Tokio or MCP SDK is needed.

```text
Agent --MCP (stdio)--> titan_mcp --BRP (HTTP JSON-RPC)--> game
```

## Enable BRP in your game

Enable the `bevy_remote` feature on your Titan/Bevy dependency, then add:

```rust,ignore
use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RemotePlugin::default())
        .add_plugins(RemoteHttpPlugin::default()) // 127.0.0.1:15702
        .run();
}
```

Components, resources, and input messages must be reflected and registered for
BRP access. Custom components generally need `#[derive(Component, Reflect)]`,
`#[reflect(Component)]`, and `app.register_type::<YourComponent>()`.
Keyboard injection needs the keyboard input plugin and registered
`KeyboardInput` and `WindowEvent` messages. Each phase is delivered to both
standalone keyboard readers/`ButtonInput` and ordered window-event readers.
Mouse injection needs the usual window/input/picking
plugins and registered `CursorMoved`, `MouseButtonInput` and `WindowEvent`
messages. Clicks first update the target window's physical cursor position,
using its effective scale factor (including any override), then send standalone
cursor/button messages for raw readers and `ButtonInput` as well as aggregate
window messages for picking. Cursor deltas use the previous in-bounds physical
position and effective scale factor; first entry reports no delta, and stationary
moves report zero. A native window backend may also move the OS cursor.

For pause, resume, deterministic step, status, and fast file-based screenshots,
add the optional `TitanRemotePlugin` from `titan_remote` alongside these plugins:

```rust,ignore
use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin};
use titan_remote::TitanRemotePlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            RemotePlugin::default(),
            RemoteHttpPlugin::default(),
            TitanRemotePlugin,
        ))
        .run();
}
```

Enable `titan_remote`'s `render` feature for screenshot methods. Its time control
requires `TimePlugin` and `FrameCountPlugin`, both included in `DefaultPlugins`.
For a windowed setup with Titan Remote, use `titan_remote`'s server example at
the end of this README. The `titan_mcp` server example below is BRP-only and
headless. Without Titan Remote, standard BRP tools still work, screenshot uses
the BRP observer fallback when a renderer is present, and time-control tools
explain how to enable the missing methods.
Pausing virtual time does not stop systems that ignore time.

## Build and connect Claude Code

From this repository:

```sh
cargo build -p titan_mcp
```

Paste this into your project's `.mcp.json`, replacing the executable path with
an **absolute path** to your build:

```json
{
  "mcpServers": {
    "titan": {
      "command": "/absolute/path/to/titan/target/debug/titan_mcp",
      "args": ["--url", "http://127.0.0.1:15702"]
    }
  }
}
```

Start the game, then start Claude Code in that project. Ask it to use
`game_status`, find a reflected type with `find_types`, query entities, and take
a screenshot. Other MCP clients can use the same executable/arguments.

Configuration priority: `--url` overrides `TITAN_BRP_URL`, which overrides
`http://127.0.0.1:15702`. `--help` prints usage. The URL may include a BRP path.
MCP uses one JSON-RPC message per line, with `initialize`, `tools/list`,
`tools/call`, notifications, and `ping`; stdout is reserved for MCP.

## Launch, rebuild, and restart a game

Without `--game-cmd`, the server stays attach-only (the default above). It never
stops an attached game. To let the agent manage a game, configure **fixed JSON
argv arrays**, not shell command strings. Arguments are literal: spaces in paths
work without shell quoting, and pipes, substitutions and redirects are not
interpreted. The agent cannot supply commands, working directories, or extra
arguments through a tool. Configuring a shell executable explicitly is trusted
operator configuration, not a sandbox.

For the crate-local GPU-free BRP example, build once:

```sh
cargo build -p titan_mcp --example server
```

Then use this `.mcp.json` (replace all absolute paths):

```json
{
  "mcpServers": {
    "titan": {
      "command": "/absolute/path/to/titan/target/debug/titan_mcp",
      "args": [
        "--url", "http://127.0.0.1:15702",
        "--game-dir", "/absolute/path/to/titan",
        "--game-cmd", "[\"/absolute/path/to/titan/target/debug/examples/server\"]",
        "--build-cmd", "[\"cargo\",\"build\",\"-p\",\"titan_mcp\",\"--example\",\"server\"]",
        "--ready-timeout-secs", "30",
        "--stop-timeout-secs", "3",
        "--build-timeout-secs", "300"
      ]
    }
  }
}
```

Use `launch_game`, `get_resource { "resource": "Counter" }`,
`restart_game { "rebuild": true }`, then query `Counter` again. `rebuild_game`
also stops the game and builds, but leaves it stopped until `launch_game`.
`restart_game` without `rebuild` only stops and launches. A second launch of an
owned running game is rejected; `stop_game` is idempotent in configured mode.

### Doom demo

Doom serves BRP only with its opt-in `remote` feature. Build first:

```sh
cargo build -p titan_doom --features remote
cargo build -p titan_mcp
```

Use this `.mcp.json`, replacing every absolute path:

```json
{
  "mcpServers": {
    "titan": {
      "command": "/absolute/path/to/titan/target/debug/titan_mcp",
      "args": [
        "--url", "http://127.0.0.1:15702",
        "--game-dir", "/absolute/path/to/titan",
        "--game-cmd", "[\"/absolute/path/to/titan/target/debug/titan_doom\",\"--brp-port\",\"15702\"]",
        "--build-cmd", "[\"cargo\",\"build\",\"-p\",\"titan_doom\",\"--features\",\"remote\"]"
      ]
    }
  }
}
```

The demo always binds to IPv4 loopback. `--brp-port` defaults to 15702; match it
to `--url` and reserve an unused port. Remote builds start paused and disable
the human gameplay input adapter so idle/unfocused windows cannot erase agent
actions. The default build has no BRP listener. Cargo must be on the sidecar's
PATH (or use an absolute path); Windows binaries have an `.exe` suffix. Respect
any custom `CARGO_TARGET_DIR` in executable paths. Prefer the prebuilt command
above; using `["cargo","run","-p","titan_doom","--features","remote"]` instead
requires raising `--ready-timeout-secs` to include compilation.

Walkthrough:

1. `launch_game {}`, then
   `get_resource {"resource":"titan_doom::PlayerState"}` (initial tick zero).
   `get_resource {"resource":"titan_doom::combat::CombatState"}` exposes combat,
   stable object IDs, and latest-tick events. This is debug-mode inspection, not
   screenshot-only playtesting.
2. `set_resource {"resource":"titan_doom::GameplayActions","value":
   {"movement":[0,1],"look_delta":[0,0],"fire":false,"interact":false,"restart":false}}`.
   Drive this shared action API rather than writing gameplay state or raw input.
3. `step {"frames":6}`, then query `PlayerState`: six playing ticks and 0.3
   units of unobstructed movement. Clear held movement with `set_resource`
   before advancing again. `step` advances app frames, not fixed ticks: the
   default 1/60-second dt matches Doom's 60 Hz fixed clock; other dt values can
   produce zero or multiple ticks per frame, carrying fractional overstep.
4. `screenshot {}` (keep the native window visible), then `stop_game {}`.
   `render + remote` enables Titan's screenshot fast path automatically.

See the [demo README](../../demos/doom/README.md#agent-control-over-brp-opt-in)
for action consumption, pause/restart semantics, fixed-step timing, and headless
verification. BRP is unauthenticated: use only trusted local agents and stop the
game when finished.

### Lifecycle limits

Timeout flags accept integer seconds in `1..=3600`; defaults are 30/3/300.
Readiness checks valid BRP `rpc.discover` responses within one shared deadline,
including HTTP reads. A failed launch stops the newly launched tree. Launch
refuses an already responsive BRP endpoint rather than claiming an existing
game. Reserve a unique loopback port for this server: BRP has no authentication
or process-identity handshake, so a concurrent external listener can still race
with launch.

**Rebuild stops first.** A failed/timed-out build leaves the old game stopped
and does not launch a new one; fix the code and retry. A missing build command
is rejected *before* stopping the game. Compiler output is captured from both
stdout and stderr concurrently, with 4 KiB of prefix and 4 KiB of tail per
stream and explicit middle-byte omission counts. The tail retains compiler
errors emitted after long dependency-build progress logs. Use Cargo's default human-readable diagnostics (optionally
`--color=never`); JSON diagnostic lines are returned as text, not parsed. Build
success returns process state, not logs. Game stdout/stderr are currently
discarded (log access is tracked in #62), and game stdin is closed; no child
output can corrupt MCP stdout or fill an unread pipe.

`game_status` adds a `process` object: `configured`, `owned`, `state`, live
command `pid`, and last `exit` (`code`, `success`, `description`). States are
`attached`, `stopped` (never launched), `running`, and `exited` (including an
intentional stop). The PID can be Cargo's PID. Managed status works even when
BRP is unreachable, returning `reachable: false` and the BRP error; attach-only
status retains its previous connection-error behavior. After an owned game
crashes, world tools report its exit status instead of a generic connection
error. A fresh successful spawn clears the previous exit.

Unix commands run in a private process group: stop sends SIGTERM, waits the
full grace period (even if the command leader exits), then sends SIGKILL to
remaining descendants. The leader is observed without reaping until after the
last group signal, preventing PID/group identity reuse during cleanup. Windows commands
start suspended into a Job Object; stop makes a bounded best-effort
`taskkill /T` request, then terminates the job (console apps may not support
graceful shutdown). Builds get the same tree isolation. Commands must not
intentionally detach/escape their group or hand inherited pipes to unrelated
processes. Normal stdio EOF, protocol I/O errors, and dropping the process
manager stop its game. Ctrl-C and Unix SIGTERM/SIGHUP request cooperative
shutdown, interrupt idle stdin, blocked stdout, and readiness/build polling,
then clean up the owned tree. The binary uses bounded 8 KiB stdio chunks and
blocking I/O workers; each response's output chunks share a ten-second
backpressure deadline, and shutdown never joins a worker stuck in OS I/O. In-flight BRP calls finish within their existing bounded timeout.
Power loss, Unix SIGKILL, and other abrupt termination cannot run Rust cleanup;
call `stop_game` before forcibly terminating the sidecar. Never run two lifecycle
managers against the same game/port.

### Library API for follow-up tools

`process::{CommandSpec, ProcessConfig, ProcessManager}` separates trusted
configuration from lifecycle operations. `launch`, `stop`, `rebuild`, `restart`,
`status`, and `check_game` are synchronous and require mutable access, so callers
serialize them with other game operations. `tools::call_managed` and
`protocol::serve_managed` take that manager; the original `call`/`serve` APIs
remain attach-only wrappers. The caller owns the manager and drops it on
shutdown. `cancellation()` returns a cloneable, one-way `ProcessCancellation`
handle for shutdown from another thread; cancellation forbids new launches/builds
but does not replace dropping/stopping the manager. Log/fuzz/playtest tooling can build on this API without granting MCP
callers a command-execution interface.

## Tools

Use `tools/list` for the authoritative JSON input schemas.

| Tool | Purpose |
| --- | --- |
| `game_status` | Process ownership/PID/exit, reachability, discovered BRP methods, optional Titan status |
| `launch_game`, `stop_game` | Start or stop only the configured, owned game tree |
| `rebuild_game` | Stop, run the fixed build command, and stay stopped |
| `restart_game` | Stop, optionally `rebuild: true`, then launch and wait for BRP |
| `query_entities` | Fetch components with `with` / `without` filters |
| `get_components`, `list_components` | Read entity components or list their types |
| `set_component` | Mutate a reflected component field by path |
| `insert_components`, `remove_components` | Insert/remove entity components |
| `spawn_entity`, `despawn_entity` | Create/destroy entities |
| `list_resources`, `get_resource`, `set_resource` | Inspect/mutate world resources |
| `find_types` | Search registry type paths by substring, without dumping the registry |
| `send_key` | Keyboard `press`, `release`, or `tap` |
| `click` | Cursor move followed by mouse press/release at logical coordinates inside the window |
| `screenshot` | PNG MCP image content, fast Titan path or BRP observation fallback |
| `pause`, `resume`, `step` | Titan time control; step waits for completion |
| `brp_call` | Raw BRP method and params escape hatch |

Type arguments accept exact full paths or short names such as `Transform`.
Ambiguous names fail with candidate paths; use one of those full paths.
Unknown types explain reflection/registration requirements. `find_types` uses
case-insensitive substring search. Entity IDs are BRP's numeric entity IDs,
not an index extracted from a debug string.

Text results are compact JSON. Large results are replaced with valid JSON
containing truncation metadata, including omitted item counts for arrays and
advice on narrowing queries. Compacted `items` envelopes retain small sibling
fields (including continuation tokens); omitted/overwritten sibling fields are
explicitly counted. Carried omission counts saturate with a flag if their sum
cannot fit in `u64`. Errors include BRP codes/messages and next steps.
Tool-error text is also capped at 24 KiB: oversized errors preserve a UTF-8-safe
diagnostic prefix with explicit truncation/omitted-byte metadata and guidance.
HTTP responses, screenshot sizes, and waits are bounded. A game restart may
invalidate previously obtained entity IDs; query again after reconnecting.

Keyboard taps and clicks use frame barriers when `titan.status` is available,
including one after the release, so a follow-up query or screenshot sees the
input fully processed.
Each barrier's baseline and status polls share a three-second deadline; failed
barriers still attempt releases after a delivered press. Partial press delivery
also triggers best-effort release in both input channels. Individual release
requests retain the usual bounded HTTP timeout, outside the barrier budget.
Vanilla BRP has no frame barrier, so input phases use a best-effort 100 ms delay;
a game updating slower than 10 Hz may need separately timed press/release calls.
Titan's frame counter continues while paused and wraps at `u32::MAX`; input
barriers use wrapping arithmetic. `step` waits for `pending_steps` to reach zero
with virtual time paused, rather than comparing numerically ordered frame IDs.
Use a single time controller: another client's `pause` or `resume` can cancel a
pending step. `dt_secs` defaults to `1/60`, must be in `(0, 1]`, and must round to
at least one nanosecond. Step submission and completion polling share a
15-second deadline after method discovery; a long step may continue in the game
after a client timeout, so use `pause` to cancel it if needed.

Screenshots use the primary window. `timeout_secs` defaults to 10 (maximum 60)
and covers the BRP capture sequence; best-effort cleanup may take another
100 ms. The fast path requests a `.png` inside a private temporary directory,
polls Titan's screenshot token until publication, and securely opens the exact
requested destination. Both atomic replacement and legacy in-place writes are
supported. Both platforms open relative to the held directory handle: Unix uses
`openat` without following symlinks or blocking on FIFOs; Windows uses a safe
`cap-primitives` wrapper around handle-relative `NtCreateFile` and rejects
reparse points on the opened handle. Windows shares parent write access for
atomic publication while denying parent deletion/replacement. Byte, decoded-image, checksum and deadline checks apply,
and the temporary directory is removed on ordinary success/error paths. The
local temp-directory ancestors are trusted. Animated PNGs are rejected. OS I/O
and individual decoder operations cannot be forcibly interrupted; deadlines are
checked between them. If Titan advertises screenshot methods, their operational
errors are returned without retrying through raw BRP: this preserves the server's
limit on pending or stuck readbacks. Only games without those methods use the
fallback, which observes a fresh empty entity,
waits for ECS observer registration, then inserts `Screenshot`, so capture
cannot race ahead of the observer. Its deadline-bounded reader is joined on
all exits; an early capture failure may wait for the remaining budget rather
than leave a background reader running. The fallback needs reflected/registered
`Screenshot` and `ScreenshotCaptured` types and a renderer. On platforms that
stop rendering occluded windows, keep the window visible. The file-based Titan
path assumes the game and sidecar share the local filesystem.

On a fallback failure after inserting `Screenshot`, cleanup removes that
component and despawns the entity only if the renderer hasn't started its
readback. Bevy despawns renderer-owned captures itself, and despawning one
early would crash the game when the late capture arrives. Upstream BRP keeps a
small bookkeeping entry for every entity-scoped `world.observe` and has no way
to unobserve, so each fallback screenshot permanently grows the game's memory
by a few bytes. Long sessions should add `TitanRemotePlugin`, whose fast path
avoids this.

## Localhost only: development, not a security boundary

Only plain HTTP loopback addresses are accepted; `localhost` is pinned to
`127.0.0.1`. Proxy use and redirects are disabled. There is **no authentication,
TLS, remote access, or sandbox**. BRP grants powerful world/file access and
`brp_call` intentionally exposes arbitrary game methods. Use only with trusted
agents and development games. Keep the game's BRP listener bound to loopback;
do not expose it on a network. Stop BRP-enabled games when finished.

## Verification

```sh
cargo test -p titan_mcp
# Real render-gated screenshot handlers, without a window or GPU:
cargo test -p titan_mcp --all-features --features bevy_remote/bevy_render
cargo clippy -p titan_mcp --all-targets --all-features --features bevy_remote/bevy_render --no-deps -- -D warnings
cargo run -p ci -- format
cargo run -p ci -- clippy
cargo run -p ci -- test
cargo run -p ci -- doc-check
```

The integration tests launch a real headless Bevy app in a child process with
`RemotePlugin` and `RemoteHttpPlugin` on an OS-selected free port. Killing and
waiting on the fixture process releases the detached HTTP listener. Protocol
tests pipe JSON through the built binary. Time-control tests use the real
`TitanRemotePlugin` and verify actual virtual-time deltas, paused input, and
counter rollover. Controlled loopback HTTP tests stall baseline/poll responses
at every input barrier, including the post-release one, and verify the shared
deadline and cleanup phases.
The `remote-render` test feature enables real screenshot
handlers with synthetic GPU readback: the server publishes the PNG atomically,
and MCP returns its decoded pixels. The shared Titan workflow discovers
`titan_*` workspace crates and runs all-feature tests on Linux, including MCP's
`bevy_remote/bevy_render` feature combination. No per-crate workflow is needed.
Renderer dependencies are dev-dependencies, not default sidecar runtime
dependencies. Lifecycle tests exercise the crate-local headless example code in
child processes, with no compilation inside the readiness deadline. They query
before/after rebuilding restarts, verify real rustc compiler diagnostics, crash
exit codes, bounded dual-pipe output, readiness/build timeouts, attach-only
safety, argument rejection, owned descendants/inherited build pipes, descendant
graceful shutdown, cooperative cancellation, and cleanup on stdio EOF/SIGTERM
(including an unread, backpressured stdout pipe). They do not modify or rely
on the timing-sensitive frame-count fixtures tracked in #73.

To reproduce the visual agent workflow:

```sh
cargo run --example app_under_test --features "bevy_remote bevy_feathers"
```

Connect Claude Code with the config above. Ask it to query `UiGlobalTransform`
with a `FeathersButton` filter, query `Window` for its entity and scale factor,
divide the physical button center by the scale factor, and call `click`. A
successful click logs `Button pressed!` and exits the example. Take a
`screenshot` before clicking for visual evidence.

To exercise the merged Titan Remote methods through the same MCP connection:

```sh
cargo run -p titan_remote --features render --example server
```

Ask the agent to pause, take two screenshots (the square should stay unchanged),
step 30 frames with `dt_secs: 1/60`, take a changed screenshot, then resume and
confirm motion. Keep the native window visible while capturing. This uses the
real screenshot token/status fast path, not the reflected-pixel fallback.
