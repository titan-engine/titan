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
rendering evidence. These construction checks predate transport; production remains disabled.

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
No extraction or processing runs in this increment. Existing RPG/arena reference
checksums and the committed README preview are unchanged.
