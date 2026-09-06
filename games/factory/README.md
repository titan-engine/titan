# Titan factory

A standalone 12×8 factory construction game using public Titan crates. Native,
headless and WASM players share `src/game.rs`. This implements the construction
foundation of [#89](https://github.com/titan-engine/titan/issues/89) and deterministic
transport in [#90](https://github.com/titan-engine/titan/issues/90), following the
[approved factory rules](../../docs/factory-slice.md). Production is not implemented:
normal runs start empty and advancing time creates no items. Explicitly seeded
test fixtures exercise transport without adding an item-injection player action.

## Build and play

From this directory, with stable Rust and Python 3 installed:

```sh
cargo run --bin play
cargo run --bin titan-factory
cargo run --bin titan-factory -- --sequence tests/construction.json
python3 scripts/build-macos-app.py --name "Titan Factory" --bundle-id dev.titan.factory
```

The headless default prints initial state and writes `target/titan/capture.ppm`.
The sequence runner prints each operation's result/rejection and final state.
It accepts a JSON array, at most 4096 operations and 1 MiB. Rejections are results,
so inspect `outcomes` rather than interpreting successful process exit as every
construction operation having succeeded. Malformed files fail before execution.
The unsigned macOS app is for local development only.

For the actual-WASM browser player, install Node.js for verification, then run:

```sh
python3 scripts/build-browser.py
python3 -m http.server 8080 --bind 127.0.0.1 --directory web
```

Open [the player](http://localhost:8080/play/) or the separate
[inspector](http://localhost:8080/inspector/). WebGPU or WebGL2 with floating-point
color attachments is required. The build helper installs the matching bindgen
CLI and WASM target when needed. No frontend package manager is required.

## Player controls

Click a tile to place the selected structure; right click to inspect. Keys 1, 2,
and 3 select conveyor, extractor, and processor. Q changes the placement facing,
E rotates the hovered structure, and X removes it. WASD/arrows pan; the mouse
wheel zooms. R restarts (Escape closes the native window). The native title bar
shows the selected tool and operation feedback. The browser also provides
buttons for tile actions, pan, zoom, pause/resume and restart. Construction works
while paused. Browser selection and operation results appear beside the canvas.

For bounded native GPU acceptance, run `cargo run --bin play -- --test-construction`.
It places three kinds through physical-to-logical pointer mapping, rotates,
inspects, removes, verifies fixed delivery rejection, pans and zooms, prints the
resulting state and presents two GPU frames. Add `--frames 600` to inspect it.
After building/serving the browser, open
[construction acceptance](http://localhost:8080/play/test.html). The page drives
the actual WASM/GPU player through DOM events, checks camera and resized-canvas
mapping, and reports PASS or FAIL with state. Browser GPU support is required.

## Construction contract

The initial conveyor tool faces east. The ore deposit is at (1,3); extractors
must occupy it. Conveyors and processors require an empty non-deposit tile.
The delivery at (10,3) is fixed. Place never replaces a structure. Rotate turns
clockwise; remove and rotate reject empty tiles and delivery. Disconnected and
out-of-bounds-facing outputs are legal. Each structure occupies one tile.
Inspection describes its input faces and output direction.

Commands use integer grid coordinates and facing `N`, `E`, `S`, or `W`:

```json
[
  {"op":"place","kind":"extractor","x":1,"y":3,"facing":"E"},
  {"op":"place","kind":"conveyor","x":2,"y":3,"facing":"E"},
  {"op":"rotate","x":2,"y":3},
  {"op":"inspect","x":2,"y":3},
  {"op":"advance","ticks":60},
  {"op":"remove","x":2,"y":3},
  {"op":"restart"}
]
```

Operations execute in order at safe boundaries. Rejected construction changes no
simulation state; later operations still execute. Restart reconstructs the grid,
resets camera/tool/pending input and game tick, preserving the host frame clock.
The sequence is a verification format, not a save file or general scene format.
The `sequence` protocol command accepts at most 256 operations per request;
`query recording` retains the latest 256 operations and reports the dropped count.
Each operation is limited to 4096 JSON bytes. `query tile --arguments '{"x":2,"y":3}'`
returns terrain and structure ports without mutation.

## Conveyor transport

Each conveyor holds at most one ore or plate and sends toward its facing neighbor.
It accepts inputs on its other three faces, allowing corners and competing feeds.
Head-on outputs do not connect. Processor inputs accept ore from their rear;
delivery accepts plates from the west. Extractors have no input. Machine slots
participate in transport, but extraction and processing do not run in this increment.

Each 60 Hz tick reads a snapshot of slots and ports. Only snapshot-empty receiving
slots have room, even when their old item leaves that tick. Eligible transfers
reserve destinations in source tile `(y,x)` order and commit together. A received
item moves no more than one tile per tick. A packed cycle stays jammed; a partly
occupied cycle can circulate. Fixed priority can starve a competing source.

Inspection distinguishes empty outputs from missing neighbors, incompatible input
faces, rejected item types, full destinations and contention, in that precedence
order. Item markers occupy their reported tile/slot positions; rendering never
advances transport. Rotating preserves contents and changes the next connection.
Removing an occupied structure explicitly counts its contents as discarded.
Restart starts a fresh empty accounting epoch. Tests check seeded items equal
remaining items plus deliveries and explicit discards at every boundary.

Transport acceptance uses named, bounded fixture constructors, separate from
normal construction commands. Run the native player with
`cargo run --bin play -- --test-transport`; it holds each loop position for 30
presented frames. After building and serving WASM, open
[transport acceptance](http://localhost:8080/play/test-transport.html) to compare
four GPU canvases at exact ticks with their inspected positions. The actual-WASM
harness also compares complete native and WASM states after each fixture operation.
For headless fixture inspection, use
`cargo run --bin titan-factory -- --transport-fixture cycle_partial --sequence FILE`,
where FILE contains the same ordered operation array shown above. Restart in a
fixture returns to the normal empty challenge.

## Native control

From the repository root, build `cargo build -p titan-cli`. From this directory:

```sh
cargo run --bin titan-factory -- --serve --instance factory --allow-mutation --run-for-ms 120000
```

In another terminal from this directory:

```sh
../../target/debug/titan --format json --instance factory query state
../../target/debug/titan --format json --instance factory commands
../../target/debug/titan --format json --instance factory invoke place --arguments '{"kind":"extractor","x":1,"y":3,"facing":"E"}'
../../target/debug/titan --format json --instance factory invoke construct --arguments '{"op":"rotate","x":1,"y":3}'
../../target/debug/titan --format json --instance factory invoke sequence --arguments '{"operations":[{"op":"place","kind":"conveyor","x":2,"y":3,"facing":"E"},{"op":"advance","ticks":60}]}'
../../target/debug/titan --format json --instance factory query recording
../../target/debug/titan --format json --instance factory capture
```

Use `--project /absolute/path/to/games/factory` when invoking outside this
directory. If `CARGO_TARGET_DIR` is set, use its CLI binary. `entities` and
`entity INDEX GENERATION` expose registered construction components. Discover
metadata rather than hardcoding Rust-qualified names. Browser inspection starts
read-only; enabling controls explicitly starts a controlled instance. Construction fields are read-only; edits use validated game commands. Discovery credentials are private;
use sanitized CLI output as evidence. SIGTERM/Ctrl-C removes registration.

## Source and checks

- `src/game.rs`: authoritative validation, ECS construction, camera mapping,
  rendering, inspection and ordered operation records.
- `src/game/transport.rs`: fixed-tick snapshot transport, slots, item accounting,
  stall reasons and named test fixtures.
- `src/main.rs`: headless sequence execution and native control server.
- `src/bin/play.rs`: native window/pointer/keyboard adapter.
- `src/browser.rs`, `web/play/`: actual-WASM player and browser controls.
- `tests/construction.json`: shared native/WASM construction fixture.
- `scripts/transport-acceptance.mjs`: independent transport traces and full
  native/actual-WASM parity, called by `scripts/test-browser.mjs`.

Run from the repository root:

```sh
cargo fmt --manifest-path games/factory/Cargo.toml --all --check
cargo test --manifest-path games/factory/Cargo.toml --all-targets
cargo clippy --manifest-path games/factory/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/factory/scripts/test-control.py
python3 games/factory/scripts/build-browser.py
node games/factory/scripts/test-browser.mjs
node --test games/factory/web/inspector/*.test.mjs
node --test games/factory/web/play/*.test.mjs
```

The native harness uses bounded build/runtime processes and retains sanitized
failure diagnostics under the repository's `target/acceptance-failures`.
Node executes real compiled WASM for simulation and protocol checks; it does not
prove browser GPU rendering. Player verification is documented with its [runtime evidence](evidence/README.md). Existing RPG and arena reference images/checksums are unchanged.
