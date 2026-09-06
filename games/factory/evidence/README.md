# Factory runtime evidence

Verified on 2026-09-06, macOS Apple Silicon, using public Titan APIs.

- `native-acceptance.json`: `cargo run --bin play -- --test-construction`
  passed construction assertions and then presented two GPU frames. Assertions
  cover all three tools, occupancy rejection, clockwise rotation, inspection,
  removal, fixed delivery rejection and pointer mapping after camera pan/zoom
  and nonuniform physical surface resizing.
- `browser-acceptance.json`: `/play/test.html` passed in the Codex in-app browser
  with actual compiled WASM and GPU canvas rendering. It exercises DOM pointer
  events, controls, camera/CSS resize mapping and reset. Host frame count depends
  on startup timing; construction state and explicit fixed-tick assertions are exact.
- `native-construction.png` and `browser-construction.png`: visually inspected
  native window and browser canvas after placing an extractor on the deposit
  and rotating a conveyor south. Input ports are cyan; outputs are yellow.
  These are evidence of GPU presentation, not exact cross-platform references.

The browser inspector was also exercised through its visible JSON command form:
`construct` place, tagged rotate, and single-field `select` succeeded. Native
CLI and Node actual-WASM harnesses both ran `tests/construction.json` and checked
matching structure states, invalid-operation rejection, capture and restart.
Node is semantic/WASM evidence; the browser test and screenshots supply player
rendering evidence. These construction checks predate transport and production.

`cargo test --test render -- --ignored` passed on Apple M5 Pro / Metal.
Eight readbacks (four camera transforms × unorm/sRGB targets) matched software
RGBA exactly, maximum channel error 0 with allowed tolerance 1. The fixture
contains all structure kinds, directional ports, hover and viewport clipping.
Capture did not advance or mutate the game.

## Deterministic transport

Verified on 2026-09-06 on macOS Apple Silicon for issue #90.

- `native-transport.json` retains the first five state snapshots logged after
  successful native GPU presentations of `play --test-transport`. One seeded ore
  follows (2,2), (3,2), (3,3), (2,3), (2,2); every state conserves its one item.
  `native-transport.png` shows the actual native window with ore at (3,2).
- `browser-transport.json` is the PASS result displayed by
  `/play/test-transport.html` in the Codex in-app browser. Four actual WASM/GPU
  players are frozen at ticks 0–3. `browser-transport.png` shows the lower two
  canvases with the ore at (3,3) and (2,3), matching the reported positions.
- The native GPU transport test checks all five loop positions against software
  rendering for unorm and sRGB targets, allowing at most one byte per channel.
  These checks and the existing construction readbacks pass on native Metal.
- `scripts/test-browser.mjs` executes compiled WASM and compares complete native
  and WASM state at 86 operation boundaries across eight runs, with independent
  expected traces for one-hop flow, snapshot backpressure, contention, cyclic
  circulation/jam, disconnected outputs, machine ports and edited networks.
  Both item types, rejection atomicity, discard accounting and restart are checked.

The Rust tests additionally check source ordering independent of entity allocation,
port/type/capacity reason precedence, occupied rotation, counter overflow rejection,
machine input and output transfers in the same tick, and completion freeze.
The factory package's native control, construction and browser host tests pass.
Workspace formatting/tests/Clippy/WASM and existing RPG native/WASM control,
replay and asset checks (including native GPU playback/assets) pass unchanged.

Screenshots establish actual player presentation and inspected item movement;
they are not portable exact-pixel references. Browser execution used its default
supported GPU backend; separate WebGPU/WebGL2 transport matrix coverage is not
claimed. Named fixtures are explicit test startup setup, not injection gameplay.
No extraction or processing ran in the transport increment. Existing RPG/arena reference
checksums and the committed README preview are unchanged.


## Extraction-to-delivery production

Verified on 2026-09-06 on macOS Apple Silicon for issue #91.

- `native-production.json` records seven snapshots after successful native GPU
  presentations of `play --test-production`. Tool selection and physical-to-logical
  pointer placement construct the unseeded reference route. Every presented game
  tick checks delivery timing and independent resident-item accounting. The run
  reaches Complete at tick 1269 and keeps presenting the frozen world.
- `native-production.png` is the personally inspected native app window at
  Complete, 10/10 deliveries, tick 1269. Its five remaining orange ore markers
  match the three input belts and the processor's input and in-process slots.
- `browser-production.json` contains checkpoint states and the PASS result of
  `/play/test-production.html`, using actual compiled WASM and GPU presentation
  in the Codex in-app browser. DOM tool buttons and pointer events construct the
  route; the page checks every tick, then completion freeze, rejected edits,
  restart and identical completion after rebuilding. `browser-production.png`
  is its personally inspected completed GPU canvas with the same five ores.
- `scripts/production-acceptance.mjs` compares full native and compiled-WASM state
  at 3,469 operation boundaries. Expectations are independent of the runtime:
  extraction tick 60, batch start 64 with remaining=120, queued input 124, first
  plate 184, deliveries 189 + 120*(k-1), completion 1269. Completion leaves
  extracted=15, delivered=10 and five live ores, processor remaining=116 and
  extractor progress=1. Production is skipped on the completion tick.
  Seeded edge fixtures verify output blocking, starvation, backlog, rotation,
  complete processor removal (two ore and one plate discarded), rejected edits
  and reset. Existing transport-only fixtures retain their original behavior.

Factory unit/all-target tests, strict Clippy, native control, browser control/input,
actual-WASM construction/transport/production and native Metal GPU readback gates
pass. Workspace formatting, tests, strict Clippy and WASM checks pass, as do the
existing native/WASM RPG control, replay, assets and browser inspector/input suites.
Existing RPG/arena reference checksums and README preview are unchanged.

The player screenshots show presentation and slot positions, not portable exact
pixel references. Browser production used its default supported GPU backend;
separate WebGPU/WebGL2 production matrix coverage is not claimed. Native acceptance
advances one fixed tick per presented frame for bounded verification, so its
wall-clock duration is display-dependent; normal play uses the 60 Hz accumulator.
Seeded edge fixtures are startup-only verification APIs, never player actions.
