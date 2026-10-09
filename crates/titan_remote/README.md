# Titan Remote

`titan_remote` adds game-side time control and inexpensive PNG screenshots to
Bevy Remote Protocol (BRP). It does not depend on an MCP server or modify
`bevy_remote`. The default feature set is headless; enable `render` for screenshots.

Add `TitanRemotePlugin` alongside `RemotePlugin::default()` and
`RemoteHttpPlugin::default()`. The app must also have `TimePlugin` and
`FrameCountPlugin` (included in `MinimalPlugins` and `DefaultPlugins`). Methods
are registered at plugin finish, so either order relative to `RemotePlugin` works.
When manually driving an app, call `app.finish()` and `app.cleanup()` before
`app.update()`.

## Methods

All methods use normal JSON-RPC requests to the main-world HTTP endpoint.
Results below are the JSON-RPC `result` payload, not the entire envelope.

| Method | JSON params | JSON result |
| --- | --- | --- |
| `titan.status` | omitted, `null`, or `{}` | `{ "paused": bool, "frame": u32, "pending_steps": u32 }` |
| `titan.pause` | omitted, `null`, or `{}` | status, as above |
| `titan.resume` | omitted, `null`, or `{}` | status, as above |
| `titan.step` | `{ "frames": u32, "dt_secs": f32 }` (`dt_secs` optional; default `1/60`) | `{ "target_frame": u32 }` |
| `titan.screenshot` (`render`) | `{ "path": string }` (`path` optional; params may be omitted) | `{ "token": u64 }` |
| `titan.screenshot_status` (`render`) | `{ "token": u64 }` | `{ "pending": true }` until written; then `{ "pending": false, "path": string }` |

### Time control

`pause` and `resume` pause/unpause `Time<Virtual>`. Either cancels an outstanding
step and restores its saved clock configuration. `step` advances the **next** N
app frames with `TimeUpdateStrategy::ManualDuration`, then pauses again. Poll
`status` until `pending_steps` is zero. BRP handles requests after `Last`, so the
frame in which the request arrives is not counted as a stepped frame.

Stepping temporarily uses virtual speed 1 and a maximum delta equal to the step
delta, then restores the previous speed, maximum delta, and time-update strategy.
`dt_secs` must be finite, positive, round to at least one nanosecond, and at most
one second (to bound fixed-update work per frame). Unknown fields and malformed
parameters return JSON-RPC invalid-params errors. A second step while one is
pending is rejected with application error `-32000` (`STEP_IN_PROGRESS`); cancel it
with `pause` or `resume` first. Zero frames pauses
immediately. `frame` is Bevy's `FrameCount`, including frames spent paused; it and
`target_frame` wrap at `u32::MAX`. Poll completion, rather than requiring frame
equality: the game continues rendering after the target frame.

**Known limitation:** pausing virtual time freezes time-driven behavior such as
`FixedUpdate`, timers, and movement scaled by `delta_secs`. Systems that ignore
time, rendering, BRP, and `FrameCount` still run every app frame. This is not
schedule-level pausing. A follow-up could use ECS `Stepping` for schedule control.
Application systems should not override the clock configuration while stepping.
`ManualDuration` also advances `Time<Real>` with synthetic deltas: its elapsed
value is not a wall-clock measurement after stepping. Returning to `Automatic`
can produce a zero or larger first real-time delta; normal completion absorbs it
while virtual time is paused, whereas cancelling with `resume` uses the restored
virtual maximum-delta clamp.

### Screenshots

Screenshots capture the primary window using Bevy's asynchronous screenshot
pipeline. Unlike BRP's reflected image event, only a token and a file path cross
the wire. Poll `titan.screenshot_status` for completion; **the path is returned
only after the PNG has been written**, never just because an older file exists.
The optional path is a server-local PNG path; omitted paths use a per-process
temporary directory. Relative paths resolve against the game's working directory;
parent directories must exist. Results contain absolute paths. Rendering must be
initialized and a primary window must exist. Keep that window visible while
capturing: on macOS/Metal, a fully occluded or minimized window can produce a
black image even though capture and file writing succeed.

Captures are serialized to avoid Bevy dropping duplicate primary-window captures
in one frame. At most 64 jobs are tracked; unfinished jobs time out after 30
wall-clock seconds, including while paused. Finished tokens are retained for five
minutes, but may be evicted earlier to make room. Expired/unknown tokens return
invalid-params errors; capture and disk failures return internal errors. Saved
files are not deleted when tokens expire; clients own their cleanup. The destination's
parent directory must be writable for atomic publication. Temporary staging files
and final screenshots are owner-readable/writable on Unix (mode 0600). An abandoned
GPU capture can leave an empty staging file until its observer is dropped; a crash
can leave staging files behind.

The HTTP server is an unauthenticated development tool. Keep its default
loopback binding and expose it only to trusted clients: BRP can mutate the world,
and this extension can write files with the game's permissions. Returned paths
are on the game's machine, not necessarily the client's machine. Disk encoding
uses Bevy's `save_to_disk` observer when the asynchronous capture arrives.

## Runnable example and curl walkthrough

The example lives in this crate, not the upstream root example registry:

```sh
cargo run -p titan_remote --features render --example server
```

It opens a window containing a square rotating with virtual time. Set
`TITAN_REMOTE_PORT` to select a different port if 15702 is occupied. From another
terminal (requires `curl`; `jq` is convenient for extracting tokens):

```sh
URL=http://127.0.0.1:15702
curl -s "$URL" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"titan.pause"}'
curl -s "$URL" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"titan.step","params":{"frames":10,"dt_secs":0.016666667}}'
curl -s "$URL" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"titan.status"}'
# Repeat status until pending_steps is 0.
TOKEN=$(curl -s "$URL" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":4,"method":"titan.screenshot"}' | jq -r .result.token)
curl -s "$URL" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"titan.screenshot_status\",\"params\":{\"token\":$TOKEN}}"
# Repeat screenshot_status until pending is false; open the returned PNG path.
curl -s "$URL" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":6,"method":"titan.resume"}'
```

For windowless applications, omit the `render` feature and use `MinimalPlugins`
with the two BRP plugins and `TitanRemotePlugin`. No renderer or GPU is needed
for time control. With `render` enabled in a headless app, screenshot methods return
an error explaining that the renderer or primary window is missing.

BRP needs app frames to process requests. Use a continuously updating runner/window
mode when controlling an unattended game: reactive Winit modes can sleep until
window input arrives and are not awakened by HTTP requests.
