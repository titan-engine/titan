# Browser inspection

The browser adapter runs the same procedural RPG as the native example, using
`examples/support/procedural_rpg.rs`. It executes the same protocol envelopes
through a synchronous WASM call. No simulation tick runs between requests.

Build and serve locally:

```sh
python3 scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory web/inspector
```

Open `http://127.0.0.1:8000`. The build script installs a matching
`wasm-bindgen-cli` under `target/titan/tools`, adds the Rust WASM target if
needed, and generates web and Node packages. Generated packages are ignored by
Git. Rust, Cargo, Python 3, and Node.js are used by the build and acceptance
checks; no frontend package manager is required.

The inspector starts with a paused, read-only game. It discovers capabilities,
shows named entities and command metadata, and displays software captures.
Explicitly enabling controls starts a fresh controlled game and exposes
stepping, logical input, and registered commands. Reloading returns to read-only
mode. Field mutation remains limited by the engine's reflection support.

`BrowserRuntime(false)` rejects `step`, `invoke`, `inject_input`, and `set_field`
with structured errors. This adapter policy is stronger than the engine's
field-mutation flag: registering a game command alone does not grant browser
control. The read-only capabilities list omits all write operations.

Captures are PNG data URIs in the existing `CaptureResult.artifact` field.
The checksum is computed from the uncompressed RGBA image, so native PPM and
browser PNG captures share the same exact reference checksum.

## Message bridge

The page accepts this message shape:

```js
window.postMessage({
  namespace: "titan.inspector",
  type: "request",
  envelope: {
    schema_version: 1,
    request_id: "example-status",
    request: { type: "status" }
  }
}, location.origin);
```

Responses use the same namespace, `type: "response"`, and the unchanged
protocol response envelope in `envelope`. Match responses by `request_id`.
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
changes, and schema errors. The second checks message source/origin filtering
and response correlation. Native host tests also decode the PNG and verify its
RGBA checksum. CI runs these alongside the native separate-process acceptance.

This phase provides inspection and software captures. Continuous interactive
rendering and keyboard-driven play belong to the next renderer phase.
