# Collection room

A standalone Titan game derived from the minimal starter's public-API workflow.
The same Rust simulation runs headlessly on native and actual WebAssembly. This
package owns its rules, generated meshes, camera, input, inspection and replay;
it imports no RPG support code. The optional `player` feature adds native and
browser GPU hosts; default headless builds keep GPU dependencies disabled.
Image capture remains unregistered for the later capture integration. No software 3D image is
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
frames release all actions. Names are `up`, `down`, `left`, `right`; the players map WASD/arrows to them. The host frame stays monotonic across
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
uses the shared `titan::render::three_d` boundary. The players consume the frame without putting GPU dependencies into simulation.
The `RenderFrame` extraction and `ImageAssets` resource contain the transparent
ECS progress/completion text used by the shared compositor.

```sh
cargo fmt --manifest-path games/collection-room/Cargo.toml --all --check
cargo test --manifest-path games/collection-room/Cargo.toml --all-targets
cargo clippy --manifest-path games/collection-room/Cargo.toml --all-targets -- -D warnings
python3 games/collection-room/scripts/test-control.py
python3 games/collection-room/scripts/build-browser.py
node games/collection-room/scripts/test-browser.mjs
```

The portable starter build helper emits browser and Node bindings under
`web/inspector/pkg` and `target/titan/browser-node`; the player page is at `web/play/`.
Actual WASM acceptance executes the Node bindings in a bounded child process.
Native acceptance uses bounded separate CLI/runtime processes and sanitized
failure evidence. The CPU tests cover collisions, collection, input semantics,
restart, replay and immutable extracted geometry without opening a window.


## Play natively

```sh
cargo run --manifest-path games/collection-room/Cargo.toml --features player --bin play
python3 games/collection-room/scripts/build-macos-app.py --name "Titan Collection Room" --bundle-id dev.titan.collection-room
```

The second command builds an unsigned local development app and prints its path.
WASD/arrows move, P pauses/resumes, N advances one paused tick, R restarts and
clears the replay, and Escape exits. Focus loss pauses and cancels held keys and
buffered taps. A released tap still reaches the next tick; physical aliases are
tracked separately. Playback uses the same fixed system as manual/headless play.

Use `--recording PATH` to load an exported recording paused at its origin, then
P to play it one tick at a time. Playback pauses at the exact recording end;
N can single-step it. `--frames N` limits presented frames and `--run-for-ms MS`
bounds wall-clock execution. Use the latter for unattended playback.

Add `--inspect --allow-control --project games/collection-room --instance room-player`
to expose the authenticated inspector on the exact played instance. Omit
`--allow-control` for read-only inspection. Query `state`, `recording` and `playback`;
invoke `pause`, `resume`, `restart`, or `load_replay` with a `recording` argument.
The headless instant `replay` command is disabled in players: `load_replay` and
subsequent ticks provide interactive replay. Stepping requires pause; a step
beyond a recording end is rejected. Capture reports unsupported.

## Play in a browser

```sh
python3 games/collection-room/scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory games/collection-room/web
```

Open `/play/` and click Play. Focus the canvas to move; Space pauses/resumes,
N steps, and R restarts. Host buttons export/import recordings and replay the
44-tick reference route. Imported recordings start paused; Resume plays them on
the same canvas. Controls and the ECS overlay work without inspector permission.
Check **Enable inspector control** to authorize tool-driven writes on the played
instance. The same-page `window.collectionRoom.dispatch(JSON.stringify(envelope))`
returns a Promise for a schema-2 response; runtime policy enforces the checkbox.
Reload returns to disabled control. No native discovery token is exposed.

Use `?backend=webgpu` or `?backend=webgl2` to request exactly one backend; the
ordinary page permits either. WebGL2 requires floating-point color attachments
for the existing text/sprite renderer. Unsupported adapters and GPU errors are
visible and stop graphics; reload starts a fresh GPU session. Invalid control
operations and recording imports are reported without stopping graphics.

The fixed camera and 320 × 180 text layer stretch with the surface. Pixel sizes
are bounded to 2048 per axis; zero size suspends drawing until a nonzero resize.
Long frame gaps are capped at 250 ms and input is canceled on focus loss. These
hosts target native desktop winit platforms and browser WebGPU/WebGL2; they do
not add mobile/console targets or a software 3D fallback.

## Player acceptance

```sh
cargo test --manifest-path games/collection-room/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path games/collection-room/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/collection-room/scripts/test-player.py
node --test games/collection-room/web/play/*.test.mjs
# With the browser server above, open each actual GPU test page:
# /play/test.html?backend=webgpu
# /play/test.html?backend=webgl2
cargo test -p titan-render-wgpu --test composition -- --ignored
```

The native harness requires an actual window/GPU. It drives the played instance
through the authenticated CLI, exports the 44-tick winning route, steps the
recording once, resumes interactive playback and compares semantic state.
The browser test page executes actual generated WASM and renders each fixed tick,
checking the same keyboard route, replay, focus cancellation, pause/step/restart,
control policy and zero-size suspension/resizing. Its JSON result declares the
requested backend; an unavailable backend is a failure, not a fallback pass.
The shared composition test checks linear-light alpha and UI color conversion
for sRGB and non-sRGB targets with per-channel tolerance 2, retaining expected,
actual and difference images. Exact portable 3D pixel equality is not claimed.
