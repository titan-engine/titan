# Collection room

A standalone Titan game derived from the minimal starter's public-API workflow.
The same Rust simulation runs headlessly on native and actual WebAssembly. This
package owns its rules, generated meshes, camera, input, inspection and replay;
it imports no RPG support code and has no GPU dependency. Interactive players,
HUD composition and image capture are separate work. No software 3D image is
substituted for a GPU render.

## Run and inspect

From the Titan repository:

```sh
cargo run --manifest-path games/collection-room/Cargo.toml
cargo build -p titan-cli
cargo run --manifest-path games/collection-room/Cargo.toml -- \
  --serve --project games/collection-room --instance room \
  --run-for-ms 120000 --allow-mutation
```

The first command prints the initial semantic state and exits. The server starts
paused. In another terminal:

```sh
target/debug/titan --format json --project games/collection-room --instance room entities
target/debug/titan --format json --project games/collection-room --instance room query state
target/debug/titan --format json --project games/collection-room --instance room commands
target/debug/titan --format json --project games/collection-room --instance room input 1 \
  --actions '{"right":{"kind":"button","value":true}}'
target/debug/titan --format json --project games/collection-room --instance room step 1
target/debug/titan --format json --project games/collection-room --instance room invoke restart
```

Inputs are complete button snapshots for future host ticks. Missing injected
frames release all actions. Names are `up`, `down`, `left`, `right`; a future
player can map WASD/arrows to them. The host frame stays monotonic across
restart; the game timeline and recording reset, input is cleared, and the
session generation increments. Completion stays latched until restart.

Entity inspection exposes names and position/progress fields. The `teleport`
command accepts integer millimetre coordinates, for example
`invoke teleport --arguments '{"x":-3000,"z":3000}'`. It validates room and
obstacle clearance before assignment and requires native `--allow-mutation`.
Teleport invalidates the current recording. Browser control starts disabled;
construct `BrowserRuntime(true)` explicitly to enable controls and mutation.
A rejected operation leaves the relevant game state unchanged; inspect the
response frame/revision before retrying a timed-out request.

Native inspection uses the existing authenticated discovery/control server.
Ctrl-C, SIGTERM or the supplied duration stops the owned server and removes its
registration. `--diagnostics on-failure|always|never` selects bounded diagnostic
bundles. On failure, follow `error.details.diagnostic_bundle`; it includes game
state, entity metadata and input history without image capture. Do not copy raw
discovery registrations or their bearer tokens into evidence.

## Deterministic fixture

Coordinates are right-handed, +Y up, on the XZ floor. Authoritative X/Z values
are integer millimetres; extraction converts them to metres. Axial movement is
250 mm per tick. Diagonal movement uses 177 mm per axis (250.316 mm total, within
0.13% of axial speed); opposing directions cancel. Collision is resolved in a
stable axis order. Stationary obstacles and room bounds block the player's body.

The player center is bounded to ±4500 mm on each axis; its half-width is 200 mm.
The player starts at `(-3000, 3000)`. Obstacle centers are `(0, 0)` and
`(2000, 1000)`, with half-widths 750 and 500 mm. Collectibles are at
`(-1000, 3000)`, `(-1000, -2000)` and `(3000, -2000)`. From a fresh/restarted
room, inject **right for 8 ticks, up for 20, right for 16**, one complete input
snapshot per tick. At game tick 44 the player is `(3000, -2000)`, has collected
all three objects once, and completion is latched. The scripts below execute
this fixture through both native control and actual WASM.

`query recording` exports the consumed digital input via Titan's shared
`RecordedButtons` representation. `invoke replay --arguments-file FILE` expects
`{"recording": <exported value>}`. Replay validates the fixture identity and
bounded frames before replacing the scene, reconstructs the original room,
and executes the same fixed systems. This is a bounded game-local replay
artifact, not a save-file format. Host frames/session generation are provenance,
not part of semantic final-state equality.

## Extraction and checks

`game::build_game()` returns an `App`; run `Startup`, then refresh extraction.
`app.extracted::<Result<RenderFrame3d, Frame3dError>>()` owns the resolved meshes,
fixed elevated perspective camera and lighting. Mesh data is procedural and
uses the shared `titan::render::three_d` boundary. A future host can consume the
frame without putting rendering dependencies into simulation.

```sh
cargo fmt --manifest-path games/collection-room/Cargo.toml --all --check
cargo test --manifest-path games/collection-room/Cargo.toml --all-targets
cargo clippy --manifest-path games/collection-room/Cargo.toml --all-targets -- -D warnings
python3 games/collection-room/scripts/test-control.py
python3 games/collection-room/scripts/build-browser.py
node games/collection-room/scripts/test-browser.mjs
```

The portable starter build helper emits browser and Node bindings under
`web/inspector/pkg` and `target/titan/browser-node`; this increment supplies an API adapter, not a player page.
Actual WASM acceptance executes the Node bindings in a bounded child process.
Native acceptance uses bounded separate CLI/runtime processes and sanitized
failure evidence. The CPU tests cover collisions, collection, input semantics,
restart, replay and immutable extracted geometry without opening a window.

The semantic host uses protocol schema 2. Browser clients may await
`runtime.dispatch(JSON.stringify(envelope))`; it returns one correlated Promise
without retaining the runtime borrow. `handle` remains an immediate-only
convenience for semantic calls. Native requests use the same owned dispatch and
deferred reply boundary. This package still advertises no capture capability;
collection-room GPU capture wiring belongs to #48.
