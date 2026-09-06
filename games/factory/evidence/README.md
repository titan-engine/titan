# Construction runtime evidence

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
rendering evidence. The game deliberately has no production or transport yet.

`cargo test --test render -- --ignored` passed on Apple M5 Pro / Metal.
Eight readbacks (four camera transforms × unorm/sRGB targets) matched software
RGBA exactly, maximum channel error 0 with allowed tolerance 1. The fixture
contains all structure kinds, directional ports, hover and viewport clipping.
Capture did not advance or mutate the game.
