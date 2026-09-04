# Interactive rendering

Titan renders the same procedural RPG through an exact software renderer and a
GPU sprite pipeline. The native and browser players use the same game builder,
assets, fixed-tick simulation, and input sampling helper.

## Play

Native macOS/Linux:

```sh
cargo run --example play_rpg
```

Move with arrow keys or W A S D. Close the window to exit. For a bounded native
GPU smoke test, replay the known route and present two frames:

```sh
cargo run --example play_rpg -- --replay --frames 2
```

Browser:

```sh
python3 scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory web
```

Open `http://127.0.0.1:8000/play/`, then click Play. The player provides keyboard
and pointer controls, Pause/Resume, and a reference replay button that pauses
at the completed shrine. The inspection page remains available at `/inspector/`.
Browser GPU support uses WebGPU or the WebGL2 backend; WebGL2 requires
floating-point color attachments. Unsupported graphics configurations report
an initialization error rather than silently using software rendering.

The simulation runs at 60 fixed ticks per second. Held movement repeats every
six ticks (ten tiles per second); new presses take effect at the next fixed
tick, including taps released before that tick. Host elapsed time is capped at
250 ms per update to avoid unbounded catch-up after a background pause. Logical
recorded input still applies directly at exact ticks, preserving the original
reference replay.

## Extraction and rendering

`App::add_extractor` registers an immutable snapshot builder receiving `&World`.
Snapshots are stored outside ECS resources and retrieved with `App::extracted`.
Builders run in registration order after startup, each completed fixed tick,
ordinary schedules, and explicit deferred-command boundaries. They observe
applied structural changes and the completed tick counter. Direct world edits
can refresh snapshots explicitly with `App::refresh_extracted`.

The RPG extracts a renderer-neutral `RenderFrame`. The wgpu backend consumes
that frame and the existing CPU `ImageAssets`, drawing actual textured quads
with nearest sampling, tint, alpha, clipping, and deterministic layer/order
sorting. It renders to a logical-size floating-point intermediate and presents
to the output surface with nearest scaling. Platform surface and event-loop
code belongs to the runners; `App` does not own a window or browser event loop.

See [the GPU crate](../crates/titan-render-wgpu/README.md) for API and supported
format details.

## Verification

Normal CI remains GPU-independent and compiles both runner paths. Hardware
readback tests are explicit:

```sh
cargo test -p titan-render-wgpu --test offscreen -- --ignored
TITAN_GPU_TOLERANCE=0 cargo test -p titan-render-wgpu --test offscreen completed_rpg_replay -- --ignored
```

They cover sprite semantics and the complete RPG replay. The RPG readback has
been verified on Metal against both unorm and sRGB targets with the exact
software checksum `190a92085def5677`. General alpha/tint cases allow small
per-channel rounding differences, configurable by `TITAN_GPU_TOLERANCE`.
Software captures remain the exact reference; a GPU comparison is integration
evidence and does not replace the headless semantic tests.

The browser player has also been exercised through the complete reference
route with the actual generated WASM and GPU backend.
