# Arena Survival

A standalone Titan game copied from `starters/minimal`, using only public Titan
crates. Move the cyan player with arrows/WASD, avoid coral pursuers, and survive
20 seconds. Press Space to dash in your current movement direction (or your last
direction when standing still; initially right). The dash lasts six fixed ticks
(0.1 seconds), moves four pixels per tick on each active axis and locks its
direction. It has no invulnerability. A two-second cooldown starts on activation;
release and press Space again once ready. The HUD shows readiness and cooldown.
Three contacts lose; contacts have a one-second cooldown. Press R
or the browser Restart button to reset. Browser restart pauses: press Resume.

From this directory (stable Rust, Python 3, Node.js):

```sh
cargo test --all-targets
cargo run --bin titan-game
cargo run --bin play
cargo run --bin play -- --frames 2
python3 scripts/build-browser.py
python3 -m http.server 8082 --bind 127.0.0.1 --directory web
```

Open [Play](http://127.0.0.1:8082/play/) or
[Inspector](http://127.0.0.1:8082/inspector/). Browser GPU requires WebGPU or
WebGL2 with floating-point color attachments. Native windows support macOS/Linux.
The browser inspector starts read-only; enable controls for mutations.

Dependencies are relative to this checked-in directory. For copies outside the
repository, use the path-rewrite instructions in `../../starters/minimal/README.md`
with `games/arena` as the source directory. The standalone workspace/library name
is deliberately retained as `titan-game`/`titan_game` for the copied adapters.

## Controlled inspection

From this directory, run:

```sh
cargo build --manifest-path ../../Cargo.toml -p titan-cli
cargo run --bin titan-game -- --serve --instance arena --allow-mutation --run-for-ms 120000
```

In another terminal in this directory:

```sh
../../target/debug/titan --format json --instance arena instances
../../target/debug/titan --format json --instance arena entities
../../target/debug/titan --format json --instance arena entity 0 0
../../target/debug/titan --format json --instance arena input 1 --actions '{"right":{"kind":"button","value":true}}'
../../target/debug/titan --format json --instance arena step 1
../../target/debug/titan --format json --instance arena set-field 0 0 titan_game::game::Position x --value 20
../../target/debug/titan --format json --instance arena invoke restart
../../target/debug/titan --format json --instance arena capture
```

Entity IDs and qualified field keys are discoverable through inspection; the
above IDs are stable for a fresh arena. Valid development coordinates are
x=0..153, y=18..105. Invalid values fail before assignment. Fields require native
`--allow-mutation`; it is disabled by default. Input is a complete future-tick
snapshot; absent frames release all actions. Restart clears enemies, health,
elapsed time, RNG, pending input, dash direction/cooldown and outcome; protocol frame remains monotonic.
The deterministic `dash` action accepts a button just like movement. For example,
`input 1 --actions '{"right":{"kind":"button","value":true},"dash":{"kind":"button","value":true}}'`
starts a rightward dash. Held input never automatically repeats or queues a dash;
presses during cooldown are discarded. Dash motion respects arena bounds, and
finished runs freeze all simulation including dash timers. Diagonal movement
retains the arena's existing per-axis speed convention.

`verify_survival` is a diagnostic assertion command: it succeeds only after a win.
A failed command returns a bounded diagnostic bundle with run state and input.
No token-bearing discovery files are retained as evidence.

## Verification

```sh
cargo fmt --all --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo check --lib --target wasm32-unknown-unknown
python3 scripts/test-control.py
python3 scripts/build-browser.py
node scripts/test-browser.mjs
node --test web/inspector/bridge.test.mjs
node --test web/play/*.test.mjs
cargo test --test gpu -- --ignored
```

Native control tests build the root CLI automatically. With a custom
`CARGO_TARGET_DIR`, scripts derive binary locations from Cargo metadata.
They write regenerated captures to `target/arena-evidence`. The last command
needs a GPU; all other semantic tests work headlessly.

Pinned seed: 41700 (`0xA2E4`). Initial spawn is (124,105), idle loss occurs at
game tick 310. The winning route is up30, right60, then repeat down60, left120,
up60, right120 until tick1200. It ends at (140,65), health2, spawned5, Won;
software RGBA checksum `b5cf61da6f50efd7`. Both native CLI and actual WASM verify
this route. Exact contact cooldown, pursuit, bounds, frozen outcome and restart
are also checked in focused Rust tests.

Art is generated in `src/game.rs` (pixel sprites, arena grid and tiny bitmap HUD
font). Hosts share this module; no RPG source or assets are imported. See
`../../docs/arena-exercise.md` for the independent exercise and diagnosed failure,
and [dash verification](../../docs/arena-dash.md) for dash acceptance, reviewed
images and measured iteration timings.

## macOS application bundle

From this package directory, build an app that Finder or Computer Use can open:

```sh
python3 scripts/build-macos-app.py --name "Titan Arena" --bundle-id dev.titan.arena
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
