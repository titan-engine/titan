# Minimal Titan game starter

This standalone package is a small movable sprite, with headless control, a
native window, and a browser player/inspector. Replace `src/game.rs` with your
own game. It imports public Titan crates; it does not import RPG support code.

## Copy and configure

Install rustup and Python 3.12.3; use Node.js 22.23.2 for browser checks.
The included `rust-toolchain.toml` selects Rust 1.98.1 with rustfmt, Clippy and
the WASM target even after copying outside Titan. Keep this file in your game;
rustup installs its toolchain when you first run Cargo. These are the verified
versions, not claims about minimum supported versions. Native windows/discovery are currently supported on macOS and
Linux. Browser graphics require WebGPU or WebGL2 with floating-point color
attachments. Titan is a local path dependency: crates.io publishing is disabled.

From the Titan checkout, create a game beside it:

```sh
export TITAN_REPO="$PWD"
export GAME_DIR="$(dirname "$TITAN_REPO")/my-game"
python3 scripts/create-game.py "$GAME_DIR"
cd "$GAME_DIR"
cargo generate-lockfile
cargo run --locked --bin play
```

You should see a small cyan square on a dark background. Arrow keys or WASD move
it; Escape exits. The first build downloads and compiles dependencies. Use
`cargo run --locked --bin play -- --frames 2` for a bounded window smoke check, or
`cargo run --locked --bin titan-game` for a headless run that prints game state.

Choose any new persistent directory by changing `GAME_DIR`. The setup command
refuses to overwrite an existing directory. Your game's `Cargo.toml` points to
this Titan checkout, so keep the checkout in place; if you move it, update the
Titan dependency paths in your game's manifest. The command runs from the Titan
checkout; the remaining commands run from your copied game.

The explicit `cargo generate-lockfile` step initializes your copied project's
dependency graph after configuring its manifest. Keep that game's `Cargo.lock`
in version control. Build helpers and verification use `--locked` and reject a
missing or stale lockfile; they never initialize or update it implicitly. After
an intentional dependency change, run `cargo update -p PACKAGE` (or
`cargo generate-lockfile` to deliberately resolve the whole graph), review the
lockfile diff, then run the locked checks again. Each standalone game owns its
lockfile; it does not use the engine checkout's lockfile.

## Make your first visible change

In your copied game's `src/game.rs`, find the cyan sprite color in `setup`:

```rust
Color::rgb(90, 220, 230)
```

Change it to orange:

```rust
Color::rgb(255, 160, 60)
```

Save, close the running window, and run `cargo run --locked --bin play` again. The square
should now be orange and move as before. Titan rebuilds the game when you run
Cargo; source edits do not reload into an already running player. For the
browser player, rebuild with the [browser commands below](#browser) and reload
the page after changing source.

Then try changing `DOT_SIZE` near the top of the same file from `5` to `9`.
Rebuild to see a larger square; the movement bounds use this constant too.
These edits belong to your copied game and do not change Titan's demo or
reference images. When ready, run `cargo test --locked --all-targets` and explore
[where code belongs](#where-code-belongs).

## Package layout

The standalone `[workspace]` is intentional. Keep the explicit manifest metadata
and dependency paths when copying; do not inherit Titan's workspace metadata.
The package/library name is `titan-game` / `titan_game`; changing it also requires
updating native imports. Browser builds derive the library artifact from Cargo
metadata and emit stable `titan_game` JavaScript bindings.

## Controlled native run

Build the CLI once in the Titan checkout, then launch a bounded paused runtime:

```sh
cargo build --locked --manifest-path "$TITAN_REPO/Cargo.toml" -p titan-cli
cargo run --locked --bin titan-game -- --serve --instance starter --allow-mutation --run-for-ms 120000
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
- `src/bin/play.rs`: native keyboard/window adapter using the public
  `titan_render_wgpu::SurfaceRenderer`; GPU surface setup lives in Titan.
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
cargo test --locked --all-targets
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo check --locked --lib --target wasm32-unknown-unknown
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

The build entrypoints delegate to public `scripts/titan_build.py` in the resolved
`titan` dependency. The small `scripts/titan_tools.py` loader uses Cargo metadata,
so copied games need only their normal Titan dependency paths configured; no
RPG files or checkout-relative script paths are required. Keep application names,
bundle IDs and browser binding names in the game entrypoints. The helper supports
path and Git dependencies containing the tooling file; older Titan revisions
without that file require their original scripts. See `docs/host-tooling.md` in
the Titan checkout for the helper API and verification commands.
