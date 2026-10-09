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
window messages for picking. A native window backend may also move the OS cursor.

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
For a complete runnable setup, use the crate-local server example below. Without
Titan Remote, standard BRP tools still work, screenshot uses the BRP observer
fallback, and time-control tools explain how to enable the missing methods.
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

## Tools

Use `tools/list` for the authoritative JSON input schemas.

| Tool | Purpose |
| --- | --- |
| `game_status` | Reachability, discovered BRP methods, optional Titan status |
| `query_entities` | Fetch components with `with` / `without` filters |
| `get_components`, `list_components` | Read entity components or list their types |
| `set_component` | Mutate a reflected component field by path |
| `insert_components`, `remove_components` | Insert/remove entity components |
| `spawn_entity`, `despawn_entity` | Create/destroy entities |
| `list_resources`, `get_resource`, `set_resource` | Inspect/mutate world resources |
| `find_types` | Search registry type paths by substring, without dumping the registry |
| `send_key` | Keyboard `press`, `release`, or `tap` |
| `click` | Cursor move followed by mouse press/release at logical window coordinates |
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
advice on narrowing queries. Errors include BRP codes/messages and next steps.
Tool-error text is also capped at 24 KiB: oversized errors preserve a UTF-8-safe
diagnostic prefix with explicit truncation/omitted-byte metadata and guidance.
HTTP responses, screenshot sizes, and waits are bounded. A game restart may
invalidate previously obtained entity IDs; query again after reconnecting.

Keyboard taps and clicks use frame barriers when `titan.status` is available.
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
at all three input barriers and verify the shared deadline and cleanup phases.
The `remote-render` test feature enables real screenshot
handlers with synthetic GPU readback: the server publishes the PNG atomically,
and MCP returns its decoded pixels. CI runs these tests on Linux, Windows, and
macOS. Renderer dependencies are dev-dependencies, not default sidecar runtime
dependencies.

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
