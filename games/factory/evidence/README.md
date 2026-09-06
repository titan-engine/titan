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


## Construction interface and bottleneck diagnosis

Verified on 2026-09-06 for issue #92. The implementation source revision is
`9bbac0b5ee750cd365221c5c40db9c153d191704`; the following documentation/evidence
commit does not change that source. Native Metal and the actual compiled-WASM
player ran on macOS Apple Silicon. Browser verification used the Codex in-app
browser's default supported GPU backend; a separate WebGPU/WebGL2 matrix is not
claimed.

- `native-interface.json` records the bounded `play --test-interface` assertions:
  palette clicks cannot build, keyboard focus activates the intended control,
  a wrong-facing processor is diagnosed and repaired, controlled steps complete
  at tick 1269, completed stepping stays frozen, and restart resets input/world.
  Assertions run in the native host before it presents the final inspection scene.
- `native-interface.png` shows the personally inspected native window at tick 65:
  conveyor (4,3) holds one ore, reports **Wrong facing**, names receiver (5,3),
  and explains how to rotate its input toward the source. Palette, run controls,
  selected inventory, visible discard totals, onboarding and legend fit in-window.
  Using the visible Rotate tool three times on the south-facing processor and
  clicking Step repaired it. `native-interface-repaired.png` shows tick 66,
  processor facing east, one in-process ore and **Working**. Manual R reset the
  world and selection; Escape exited the owned app.
- `browser-interface.json` is the PASS output from `/play/test-interface.html`.
  Actual DOM events build/select/edit the compiled-WASM player and prove visible
  disconnected, wrong-facing, wrong-type, full and contended causes and repairs.
  Contention names source (3,2) as the winner. It verifies exact steps, immutable
  redraws, explicit removals, held/repeated movement across pause/resume/restart
  and completion, fresh-key recovery, and scrolled-canvas focus mapping.
- `browser-interface.png` shows a personally built isolated extractor at (1,3),
  paused after 60 work ticks, with one ore in its 1/1 output and **Disconnected**
  at (2,3). A physical palette click and canvas click added that conveyor;
  a visible single-step click moved the ore and restarted extraction.
  `browser-interface-repaired.png` records that repaired UI. The paused game
  tick changed from 373 to 374; initial play time before construction accounts
  for the offset. The whole grid fits at 1280×720; its detail column scrolls.
  `browser-interface-900.png` verifies the narrower 900×700 layout. Temporary
  viewport overrides were reset and owned browser tabs closed after verification.
- `browser-interface-construction.json` preserves the existing actual-browser
  construction PASS after the UI changes, including pan, zoom, nonuniform CSS
  resize mapping, keyboard camera movement and restart.

The native host tests cover physical-to-logical grid mapping, UI hit exclusion,
keyboard aliases/repeats, focus, bounded text layout and glyph coverage across
production/transport fixtures and prospective edits. UI entities use Titan's
public `UiNode`, `UiText`, `UiButton`, `UiPointer` and `UiFocus`; no shared engine
or widget framework changed. Queries and player descriptions use one immutable
game-local model, with exact query/capture/state/recording checks in the native
CLI and compiled-WASM harnesses.

Final local gates passed: factory formatting, 29 library tests, seven native
host/UI tests, strict all-target Clippy, native control, 11 browser unit tests,
compiled-WASM construction/transport/production and explanation repair suites,
and both native Metal GPU readback tests. Full native/WASM state comparison
continues at 3,469 production boundaries and 86 transport boundaries. Workspace
formatting/tests/strict Clippy/WASM and existing native/WASM RPG control, replay,
assets and browser input/inspector tests passed. Existing RPG/arena reference
checksums and the committed README preview are unchanged. No shared APIs changed.

These screenshots prove the inspected layouts and interactions, not portable
pixel equality. Named seeded networks are test-only setup; normal play starts
empty. Browser side panels use HTML; native gameplay controls use entity UI.
Automated checks plus personal inspection provide acceptance evidence, not an
unobserved first-time human usability study.
