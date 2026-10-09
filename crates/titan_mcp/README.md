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
Keyboard injection needs the keyboard input plugin and a registered
`KeyboardInput` message. Mouse injection needs the usual window/input/picking
plugins and registered `CursorMoved`, `MouseButtonInput` and `WindowEvent`
messages. Clicks first update the target window's physical cursor position,
using its effective scale factor (including any override), then send standalone
cursor/button messages for raw readers and `ButtonInput` as well as aggregate
window messages for picking. A native window backend may also move the OS cursor.

For pause, resume, deterministic step, status, and fast file-based screenshots,
also add the optional `TitanRemotePlugin` from `titan_remote` (#17). Without it,
standard BRP tools still work, screenshot uses the BRP observer fallback, and
time-control tools explain how to enable the missing methods. Pausing virtual
time does not stop systems that ignore time.

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
HTTP responses, screenshot sizes, and waits are bounded. A game restart may
invalidate previously obtained entity IDs; query again after reconnecting.

Keyboard taps and clicks use frame barriers when `titan.status` is available.
Vanilla BRP has no frame barrier, so input phases use a best-effort 100 ms delay;
a game updating slower than 10 Hz may need separately timed press/release calls.

Screenshots use the primary window. `timeout_secs` defaults to 10 (maximum 60)
and covers the BRP capture sequence; best-effort cleanup may take another
100 ms. The fast path asks the game to write an existing `.png` file in place
and reads through its retained file handle, never an arbitrary returned path.
Atomic file replacement is not supported by that path; it may time out and
fall back to BRP observation. The fallback observes a fresh empty entity,
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
cargo clippy -p titan_mcp --all-targets -- -D warnings
cargo run -p ci -- format
cargo run -p ci -- clippy
cargo run -p ci -- test
cargo run -p ci -- doc-check
```

The integration tests launch a real headless Bevy app in a child process with
`RemotePlugin` and `RemoteHttpPlugin` on an OS-selected free port. Killing and
waiting on the fixture process releases the detached HTTP listener. Protocol
tests pipe JSON through the built binary. While #17 is unmerged, Titan methods
are covered by fixture handlers matching its contract; repeat against
`titan_remote` once available.

To reproduce the visual agent workflow:

```sh
cargo run --example app_under_test --features "bevy_remote bevy_feathers"
```

Connect Claude Code with the config above. Ask it to query `UiGlobalTransform`
with a `FeathersButton` filter, query `Window` for its entity and scale factor,
divide the physical button center by the scale factor, and call `click`. A
successful click logs `Button pressed!` and exits the example. Take a
`screenshot` before clicking for visual evidence.
