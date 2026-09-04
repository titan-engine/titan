# Minimal Titan game starter

This standalone package is a small movable sprite, with headless control, a
native window, and a browser player/inspector. Replace `src/game.rs` with your
own game. It imports public Titan crates; it does not import RPG support code.

## Copy and configure

Install stable Rust, Python 3 and Node.js. Native windows/discovery are currently
supported on macOS and Linux. Browser graphics require WebGPU or WebGL2 with
floating-point color attachments. Titan is a local path dependency: crates.io
publishing is disabled.

From the Titan checkout, copy the starter and configure its dependency paths:

```sh
export TITAN_REPO="$PWD"
export GAME_DIR="$(mktemp -d)/my-game"
cp -R starters/minimal "$GAME_DIR"
python3 - <<'PY'
import json, os, re
from pathlib import Path
repo = Path(os.environ['TITAN_REPO']).resolve()
manifest = Path(os.environ['GAME_DIR']) / 'Cargo.toml'
manifest.write_text(re.sub(r'path = "(\.\./\.\.[^"]*)"',
    lambda m: 'path = ' + json.dumps(str((repo / 'starters/minimal' / m[1]).resolve())),
    manifest.read_text()))
PY
cd "$GAME_DIR"
cargo test --all-targets
cargo run --bin titan-game
cargo run --bin play -- --frames 2
```

`cargo run --bin play` opens an unbounded playable window. Arrow keys or WASD
move; Escape exits. `--frames 2` bounds GPU presentation for smoke checks.
The standalone `[workspace]` is intentional. Keep the explicit manifest metadata
and dependency paths when copying; do not inherit Titan's workspace metadata.
The package/library name is `titan-game` / `titan_game`; changing it also requires
updating native imports. Browser builds derive the library artifact from Cargo
metadata and emit stable `titan_game` JavaScript bindings.

## Controlled native run

Build the CLI once in the Titan checkout, then launch a bounded paused runtime:

```sh
cargo build --manifest-path "$TITAN_REPO/Cargo.toml" -p titan-cli
cargo run --bin titan-game -- --serve --instance starter --allow-mutation --run-for-ms 120000
```

In another terminal, set `TITAN_REPO` and `GAME_DIR` to those same directories:

```sh
CLI="$TITAN_REPO/target/debug/titan"
"$CLI" --format json --project "$GAME_DIR" --instance starter instances
"$CLI" --format json --project "$GAME_DIR" --instance starter capabilities
"$CLI" --format json --project "$GAME_DIR" --instance starter entities
"$CLI" --format json --project "$GAME_DIR" --instance starter commands
"$CLI" --format json --project "$GAME_DIR" --instance starter input 1 --actions '{"right":{"kind":"button","value":true}}'
"$CLI" --format json --project "$GAME_DIR" --instance starter step 1
"$CLI" --format json --project "$GAME_DIR" --instance starter invoke restart
"$CLI" --format json --project "$GAME_DIR" --instance starter capture
```

If `CARGO_TARGET_DIR` is set, use its `debug/titan` instead. Native capture returns
an absolute PPM path. Stop with Ctrl-C/SIGTERM or let the time limit expire;
normal shutdown removes authenticated discovery registration.

Use `entity INDEX GENERATION` with the ID from `entities`. Its `component_fields`
contains the qualified component key, types and bounds. For example, use
`set-field INDEX GENERATION QUALIFIED_POSITION x --value 20`, substituting the
actual ID and component key. `--allow-mutation` is required at launch. An
out-of-range or wrong-type value returns `invalid_value` without assignment.
Components without registered setters cannot be edited.

Inputs are complete snapshots for exact future ticks. Once injection starts,
unspecified ticks release all actions. Restart resets game state, but the
protocol frame clock stays monotonic. Read the operation's `observed_frame` and
`state_revision`; failed operations are not transactional rollbacks. Inspect
after a timeout before retrying: transport timeouts cannot cancel running code.

## Browser

From the copied game directory:

```sh
python3 scripts/build-browser.py
python3 -m http.server 8080 --bind 127.0.0.1 --directory web
```

Open [the player](http://localhost:8080/play/) and click Play, or open
[the inspector](http://localhost:8080/inspector/). The build script installs the
WASM target and a matching wasm-bindgen CLI when needed. The inspector starts
read-only; explicitly enable controls to step, invoke, inject or edit. Browser
captures are PNG data URIs. The same-origin message bridge accepts the protocol
described in Titan's `docs/browser.md`; the native CLI does not discover browser
instances. Play and inspection are separate instances of the same game builder.

## Where code belongs

- `src/game.rs`: components, resources, action definitions, setup, fixed systems,
  generated image assets, render extraction, named entities, commands, validated
  fields, input queue, capture and diagnostic state.
- `src/main.rs`: native authenticated control queue, bounded lifecycle and
  diagnostic bundle writing. Drain requests only between simulation operations.
- `src/bin/play.rs`, `src/surface.rs`: native keyboard/window and GPU surface glue.
- `src/browser.rs`, `web/`: synchronous protocol policy, browser player and UI.

These are visible, editable host adapters, not an engine-owned application
framework. When replacing the game, preserve the small exported functions used
by hosts (or update their call sites). `InteractiveInput` samples held actions
at 60 fixed ticks per second. Keep simulation rules in fixed systems, independent
of rendering time. Use `App::add_extractor` for immutable render snapshots and
refresh extraction after direct world edits.

Register entity names with `Name`, inspection fields with
`Inspector::register_field`, commands with `register_command`, input with
`register_input_handler`, and capture with `register_capture_handler`; the game
module contains working examples. Titan's `docs/ecs-authoring.md`,
`docs/inspection.md`, `docs/rendering.md` and `docs/cli.md` describe the public APIs.

Native failures use `DiagnosticInspector` and return
`error.details.diagnostic_bundle`. Read `bundle.json`, `api.txt` and its optional
capture; accepted/rejected inputs and entity snapshots are bounded. Extend the
game-state hook for useful diagnosis. Automatic browser bundle export is not
provided. Do not include discovery bearer tokens in evidence.

## Checks

```sh
cargo fmt --all --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo check --lib --target wasm32-unknown-unknown
python3 scripts/build-browser.py
node scripts/test-browser.mjs
node --test web/inspector/bridge.test.mjs
```

From the Titan checkout, `python3 scripts/test-starter.py` copies this package
outside the repository and checks native discovery, exact input/stepping,
inspection, restart, validated edits, captures, diagnostics and clean shutdown.
Add `--browser` to also build and exercise actual WASM from the copied directory.

## macOS application bundle

From this package directory, build an app that Finder or Computer Use can open:

```sh
python3 scripts/build-macos-app.py --name "Titan Starter" --bundle-id dev.titan.starter
```

The script prints the absolute `.app` path under Cargo's target directory and
respects `CARGO_TARGET_DIR`. Open that path with Finder or select it in Computer
Use. It defaults to the `play` binary and a debug build; use `--bin NAME` or
`--release` when needed. Use distinct names and bundle IDs for separate games.
This is an unsigned local-development bundle, with no signing, notarization,
installation or security-setting changes. Bundling requires macOS; the game
itself still supports native Linux. The script follows Apple's
[macOS bundle layout](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle).
