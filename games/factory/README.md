# Titan factory

A standalone 12×8 factory construction game using public Titan crates. Native,
headless and WASM players share `src/game.rs`. This implements the construction
foundation of [#89](https://github.com/titan-engine/titan/issues/89) and deterministic
transport in [#90](https://github.com/titan-engine/titan/issues/90), and the production
objective in [#91](https://github.com/titan-engine/titan/issues/91), and the player
interface in [#92](https://github.com/titan-engine/titan/issues/92), following the
[approved factory rules](../../docs/factory-slice.md). Build an extractor on the ore
deposit, connect it to a processor, then route plates to delivery. Ten plates
complete the challenge. Normal runs start empty; seeding is only a test fixture.

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

Choose a structure from the build palette, then point at a tile to preview its
placement and output facing. Place an extractor on the brown deposit at (1,3),
connect conveyors to a processor, and connect its plate output to the fixed green
delivery at (10,3). Cyan marks are accepting input faces; yellow arrows are
outputs. Processors accept ore at their rear; delivery accepts plates from west.

Click to apply the selected tool; right click to select a tile for live inspection.
Keys 1, 2 and 3 choose conveyor, extractor and processor. Q changes placement
facing. The Rotate tool previews the next facing before applying it; E rotates
the hovered structure. X removes the hovered structure, and the Remove tool
previews the exact ore/plate discard counts. Removing in-process ore counts as a
discard even when processing has finished. Total discards remain visible until
restart. Placement never replaces an occupied tile.

Both players provide pause/resume, a single-tick step, restart, a build palette,
objective progress and live selected-tile details. Construction and inspection
work while paused. WASD/arrows pan and the wheel zooms. Restart restores the
empty challenge, initial camera/tool and cleared input. Completed runs freeze
at ten deliveries; inspection and restart remain available.

The inspection panel explains the current machine work, each slot's contents and
capacity, recipe progress, output connection and a concrete repair suggestion.
These descriptions also come from read-only `interface`, `tile` and `state`
queries; the hosts do not infer a second version of transport rules. A `preview`
query accepts integer `x`, `y` and `action` (`place`, `rotate`, `remove`, `inspect`).
Querying or rendering never advances the factory or records a construction action.

| Feedback | What to check or repair |
| --- | --- |
| Working | Extraction or processing is progressing; watch the bounded work counter. |
| Starved | Feed ore into the processor's rear input; inspect the upstream output. |
| Output blocked | An output item or finished batch is waiting; inspect its connection. |
| Disconnected | Build a receiving neighbor inside the grid, or rotate the output toward one. |
| Wrong facing | The output touches a non-input face; rotate the receiving structure or change the route. |
| Wrong item type | Process ore before delivery; processors receive ore, not plates. |
| Full destination | The receiving slot is occupied. Follow the backed-up line to its blocked end. Snapshot capacity does not free until the following tick. |
| Contended | Another eligible source wins the same empty slot in fixed top-to-bottom, left-to-right source order. Reroute a feed to avoid competition. |

An empty output is distinct from a stalled item. Connection geometry can still
warn about a missing neighbor before the first item arrives. A full line retains
all its items indefinitely; fixing its actual blocked end allows it to resume.

Native keys 4, 5 and 6 choose Inspect, Rotate and Remove. Tab moves focus among
palette and run controls; Enter activates the focused button. Space pauses or
resumes and period steps one tick while paused. The native window has an 800×560
logical interface and a minimum 1000×700 window; the game viewport retains its
384×256 coordinate system. UI clicks are consumed before world hit testing.
Browser shortcuts apply while the canvas has focus; normal buttons retain their
standard keyboard activation. Both hosts clear held input across pause/restart
and completion. Panning while paused changes only the camera.

Run `cargo run --bin play -- --test-interface` for bounded native interface
acceptance. It exercises palette hit exclusion, keyboard focus, a wrong-facing
route and its repair, controlled stepping to completion at tick 1269, completion
freeze and restart. It then presents a paused broken line for inspection; use
`--frames N` after the fixture flag to change the bounded presentation count.
After building and serving WASM, open
[interface acceptance](http://localhost:8080/play/test-interface.html). It drives
the actual DOM controls and GPU player, diagnoses and repairs five route failure
classes, and checks pause/step, repeats across reset, scrolled-canvas pointer
mapping and completion. The dedicated fixture hook exists only with `?test=1`;
it is absent from normal play. See [runtime evidence](evidence/README.md).

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
participate in transport before extraction and processing run.

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

## Production and completion

An extractor produces one ore per 60 eligible ticks (one second at 60 Hz). Its
single output pauses progress while full. A processor has separate ore input,
in-process ore and plate output slots. A batch starts with remaining=120 and
receives its first work tick on the following tick. Finished work waits at zero
if output is full. Once output frees, it emits that plate and may start queued
ore in the same tick. New output transfers no earlier than the next tick.

Build the reference route at tick zero: extractor (1,3), east conveyors at
(2,3)–(4,3), east processor (5,3), east conveyors at (6,3)–(9,3). All face east.
The first ore appears at tick 60, processing starts at 64, the first plate appears
at 184 and arrives at 189. Further deliveries are 120 ticks apart; the tenth
completes at tick 1269. Both hosts allow construction while paused or while time advances, without
changing machine rates. Acceptance fixtures build at tick zero for exact traces.

Transfer snapshot and simultaneous commit precede completion, then production.
The tenth delivery skips that tick's production and freezes the game tick,
structures and counters. The visible outcome and delivery count identify success.
Construction rejects while Complete; restart begins an empty challenge with the
initial camera/tool and no held input. Inspection remains available.

Tile inspection includes `slots`, `progress`, `remaining`, `machine_status` and
transport reason. A finished blocked batch remains an ore for accounting, even
at remaining=0. Rotation retains all contents/work; removal reports discards for
every slot, including queued and in-process ore. At every boundary, extracted
plus explicitly seeded fixture items equals all resident items plus deliveries
and explicit discards. Counter overflow stops simulation with an inspected
`diagnostic` and `Stopped` outcome; it never wraps. Restart clears that diagnostic.

`cargo run --bin play -- --test-production` constructs the reference route with
tool selection and pointer placement, presents every fixed tick through completion,
checks delivery timing and independent item counts, then holds the completed world.
Use `--frames N` to change its bounded presentation count. After building/serving
WASM, open [production acceptance](http://localhost:8080/play/test-production.html).
It constructs through DOM player controls, checks every tick, completion freeze,
rejected construction and restart, then rebuilds to complete again. The normal
player exposes no fixture injection control.

The actual-WASM harness runs `scripts/production-acceptance.mjs` automatically.
It compares complete native/WASM states at each operation and independently asserts
the specification's timing and accounting, including blocked outputs, starvation,
backlog and occupied machine edits. Named production fixtures are available only
at test startup: `--production-fixture NAME --sequence FILE` for native, and
`BrowserRuntime.production_fixture(NAME, true)` for WASM. Existing transport
fixtures keep production disabled; their restart returns to normal production.

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
- `src/game/production.rs`: extraction/processing phases and bounded production fixtures.
- `src/game/interface.rs`: immutable player explanations and prospective edit previews.
- `src/main.rs`: headless sequence execution and native control server.
- `src/bin/play.rs`: native window/pointer/keyboard adapter.
- `src/browser.rs`, `web/play/`: actual-WASM player and browser controls.
- `tests/construction.json`: shared native/WASM construction fixture.
- `scripts/interface-acceptance.mjs`: independent expected diagnoses and repairs, plus immutable query/capture checks.
- `scripts/production-acceptance.mjs`: independent production traces and accounting.
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
