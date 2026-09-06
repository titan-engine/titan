# Browser inspection

RPG pages fetch `../assets/player.png` and `../assets/tree.png` before constructing
their runtime/player. Both must succeed.
The browser build copies root `assets/` into `web/assets`; replace the served file
and reload to change art without rebuilding WASM. Startup failures expose Retry
and no playable world. See [asset behavior](assets.md) for limits and replay rules.

The browser adapter runs the same procedural RPG as the native example, through the internal
[`titan-rpg` fixture package](../fixtures/rpg/README.md). It executes the same protocol envelopes
through a safe-point WASM dispatch. Acceptance is synchronous; asynchronous
capture completion owns its immutable snapshot and releases the player borrow.

Build and serve locally:

```sh
python3 scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory web
```

Open `http://127.0.0.1:8000/inspector/` (or `/play/` for interactive GPU play). The build script installs a matching
`wasm-bindgen-cli` under `target/titan/tools`, adds the Rust WASM target if
needed, and generates web and Node packages. Generated packages are ignored by
Git. Rust, Cargo, Python 3, and Node.js are used by the build and acceptance
checks; no frontend package manager is required.

The inspector starts with a paused, read-only game. It discovers capabilities,
shows named entities and command metadata, and displays software captures.
Explicitly enabling controls starts a fresh controlled game and exposes
stepping, logical input, registered commands, and explicitly registered writable
component fields. Reloading returns to read-only mode. The RPG exposes integer
`Position.x` and `Position.y` within map bounds. Discover the fully qualified
component name through entity details before submitting a `set_field` request;
The existing host-specific names are preserved by inspector component aliases;
see the [fixture compatibility inventory](../fixtures/rpg/README.md#inspection-compatibility). Successful field
writes change the revision and subsequent capture without advancing a tick.
Invalid types or out-of-range values leave state and revision unchanged.

`BrowserRuntime(false)` rejects `step`, `invoke`, `inject_input`, and `set_field`
with structured errors. This adapter policy is stronger than the engine's
field-mutation flag: registering a game command alone does not grant browser
control. The read-only capabilities list omits all write operations.
`BrowserRuntime(true)` enables the field-mutation flag alongside the other
controls. It exposes only fields explicitly registered by the game; this is not
unrestricted component reflection.

Captures are PNG data URIs in the existing `CaptureResult.artifact` field.
The checksum is computed from the uncompressed RGBA image, so native PPM and
browser PNG captures share the same exact reference checksum.

## Reusing the browser host

The RPG, starter and arena delegate JSON handling to
`titan::inspection::BrowserSession::new(app, inspector, enable_control)` and
`session.dispatch_json(request_json)`. Construct the game and run `Startup` first;
register its commands, validated fields, input and capture hook on its inspector.
The session sets browser run mode, paused execution and the opt-in mutation
policy consistently. It owns the app and inspector so handling is exclusive.
The WASM export and same-origin JavaScript bridge remain in each game.
Export `dispatch` by returning
`titan::inspection::response_promise(session.capture_timeout(), || session.dispatch_json(request_json))`.
The closure accepts immediately, before the Promise is returned; no mutable
session borrow crosses the wait. Timer tasks (using a monotonic clock sampled
before acceptance) poll completion independently of animation frames or ticks.
The old `handle` convenience supports immediate software responses only; an
asynchronous provider requires `dispatch`. Await `runtime.dispatch(json)` for
new browser integrations, including software captures.

For an inline image capture, pass the game-rendered `Image` to
`titan_diagnostics::png_capture(&image)`. `titan-diagnostics` is consequently an
all-target dependency in copied games. It also exposes `write_png` for native
artifact destinations; native bundle permissions and size bounds remain in the
bundle writer. Games retain their rendering hooks and presentation choices.

Migration from milestone 2 removes the local JSON parsing/control-policy helpers
and PNG encoder; protocol schema 2 adds capture identity metadata; migrate request envelopes
and use the Promise-returning `dispatch` export. Reference checksums are unchanged. The standalone starter shows the complete small wrapper.

## Message bridge

The page accepts this message shape:

```js
window.postMessage({
  namespace: "titan.inspector",
  type: "request",
  envelope: {
    schema_version: 2,
    request_id: "example-status",
    request: { type: "status" }
  }
}, location.origin);
```

Responses use the same namespace, `type: "response"`, and the unchanged
protocol response envelope in `envelope`. Match responses by `request_id`; pending captures can complete after later queries.
The bridge awaits the runtime Promise and emits exactly one eventual envelope.
Only messages from the same window and the exact non-null page origin are
accepted. Cross-origin messages and messages from embedded frames are ignored.
The bridge cannot enable controls; that is an explicit action in the page.
No outgoing connection to a native development host is installed.

## Verification

```sh
python3 scripts/build-browser.py
node scripts/test-browser.mjs
node --test web/inspector/bridge.test.mjs
```

The first test executes generated WASM under Node and checks the full protocol
sequence, read-only rejection, exact replay checksum, PNG output, command
changes, valid field writes without stepping, invalid field value rejection,
and schema errors. The second checks message source/origin filtering
and response correlation. Native host tests also decode the PNG and verify its
RGBA checksum. CI runs these alongside the native separate-process acceptance.

The inspection page provides controlled stepping and exact software captures.
The [interactive player](rendering.md) uses the GPU backend and keyboard input.

## Inspecting the actual arena player

Arena's `/play/` page exposes inspection of the session rendered on its canvas.
The adjacent panel reads game state, captures a software image of that state and
exports consumed input. Inspection starts read-only. Enabling controls changes
permissions on the existing session; it does not create a replacement game.
The page uses the existing same-window, exact-origin message bridge.

`BrowserPlayer.handle` borrows its live `ArenaSession`, while local and remote
pause/restart transitions share input cancellation and clock reset boundaries.
The separate `/inspector/` page remains an isolated paused instance. Other demos
continue using that model; live integration is currently demonstrated by arena.

Browser hosts enable Titan’s `browser-capture` feature for the JavaScript Promise
and monotonic clock bridge. It is optional: raw headless WASM keeps no JavaScript
imports. Other WASM hosts provide their own elapsed-time completion driver.
