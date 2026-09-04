# Arena Survival

A standalone Titan game copied from `starters/minimal`, using only public Titan
crates. Move the cyan player with arrows/WASD, avoid coral pursuers, and survive
20 seconds. Three contacts lose; contacts have a one-second cooldown. Press R
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
elapsed time, RNG, pending input and outcome; protocol frame remains monotonic.
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
cargo test --test gpu -- --ignored
```

Native control tests build the root CLI automatically. With a custom
`CARGO_TARGET_DIR`, scripts derive binary locations from Cargo metadata.
They write regenerated captures to `target/arena-evidence`. The last command
needs a GPU; all other semantic tests work headlessly.

Pinned seed: 41700 (`0xA2E4`). Initial spawn is (124,105), idle loss occurs at
game tick 310. The winning route is up30, right60, then repeat down60, left120,
up60, right120 until tick1200. It ends at (140,65), health2, spawned5, Won;
software RGBA checksum `be61b1c710b101b6`. Both native CLI and actual WASM verify
this route. Exact contact cooldown, pursuit, bounds, frozen outcome and restart
are also checked in focused Rust tests.

Art is generated in `src/game.rs` (pixel sprites, arena grid and tiny bitmap HUD
font). Hosts share this module; no RPG source or assets are imported. See
`../../docs/arena-exercise.md` for the independent exercise and diagnosed failure.
