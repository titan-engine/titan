# Interactive rendering

The RPG player loads two loose PNGs at startup into the same engine `Image` as
procedural art. See [asset limits and iteration](assets.md) for native paths,
browser copies, bundle resources and the explicit procedural comparison.

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
at the completed shrine. It also supports [imported recording playback](rpg-replay.md)
with pause, step, restart and completion verification on the same canvas. The
isolated inspection page remains available at `/inspector/`.
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
to the output surface with nearest scaling. The public `titan_render_wgpu::SurfaceRenderer` handles default adapter/device
acquisition, surface configuration, bounded resize, and presentation. Runners
create their window/canvas and surface and pass game-owned `RenderFrame` and
`ImageAssets` references to `render`. Event loops, extraction, aspect ratio, and
input remain host/game decisions; `App` owns no window or browser event loop.

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
software checksum `f7a298f62ad75c1c`. General alpha/tint cases allow small
per-channel rounding differences, configurable by `TITAN_GPU_TOLERANCE`.
Software captures remain the exact reference; a GPU comparison is integration
evidence and does not replace the headless semantic tests.

The browser player has also been exercised through the complete reference
route with the actual generated WASM and GPU backend.

## Surface adapter migration

The RPG support adapter and the copied starter/arena `surface.rs` modules have
been replaced by `titan_render_wgpu::SurfaceRenderer`. Remove the local module
and import that public type. Construction and `resize` signatures are unchanged;
replace `renderer.render(&app)` with explicit extraction:

```rust,ignore
let frame = app.extracted::<RenderFrame>().ok_or("missing render frame")?;
let assets = app.world().resource::<ImageAssets>().ok_or("missing image assets")?;
renderer.render(frame, assets)?;
```

The return value still reports whether a frame was presented. Zero dimensions
suspend presentation, sizes are bounded to the device limit, outdated surfaces
are reconfigured, and lost/invalid surfaces return an error for the host to
handle. No RPG dependency or game-specific rendering policy enters this API.

## 3D rendering contract

The following is an agreed design, **not an implemented capability**. Execution
scope and acceptance live in [#42](https://github.com/titan-engine/titan/issues/42)
and its linked issues. As implementation lands, update this section in place
with the actual public API and evidence; do not retain a parallel design history.

### Coordinates and data

Use right-handed coordinates, +Y up, and an XZ ground plane. One world unit is
one metre; camera-local forward is -Z. Mesh front faces wind counterclockwise
when viewed from outside. Matrices act on column vectors: clip position is
projection × view × model × position. Perspective uses vertical field of view,
positive aspect ratio, finite near/far distances with `0 < near < far`, and
normalized device depth 0 at near and 1 at far. Backend adaptation must preserve
these conventions, including browser backends.

A local transform contains translation, a normalized rotation quaternion and
positive nonzero scale on each axis, composed as translation × rotation × scale.
There is no parent hierarchy in this boundary. Normals use the inverse transpose
of the model's linear part and are normalized before lighting. Reject nonfinite
values, degenerate rotations and invalid projections before GPU submission.
Start with small CPU vector/quaternion/matrix helpers for these operations in the
engine's rendering module, with convention tests; no new math dependency or
standalone general math library is selected.

A mesh owns finite positions, nonzero normals and triangle indices. Validate
matching attribute lengths, index ranges, complete nondegenerate triangles and
bounded counts/byte sizes with checked arithmetic. Cube and floor generators use
this same mesh representation. Handles are process-local and scoped to an asset
collection, with generation checks when slots are reused; replacing a collection
must never make an old numeric handle resolve to unrelated geometry. No disk
identity, importer, general asset graph or persistent GPU cache is implied.

An immutable 3D frame owns camera, lighting and ordered draw data (mesh reference,
transform and opaque base color). Its asset references retain the exact immutable
mesh versions used by the frame, so later replacement cannot alter an in-flight
render or capture. Missing/stale handles are errors. Extract from `&World` using
`App`'s existing snapshot boundary; do not put GPU objects, windows or transport
state in the frame. Fix draw order during extraction, including a stable tie
break for equal depth. CPU construction, validation and extraction work without
a GPU. The initial data API belongs beside the existing render data, with GPU
implementation in `titan-render-wgpu`; no speculative crate split is selected.

### Drawing and presentation

Use opaque triangle rendering, back-face culling, a depth attachment cleared to
1, depth writes and a strict less comparison. Start with one sample per pixel,
one directional light and bounded ambient plus Lambert diffuse lighting:
`base_linear * clamp(ambient + diffuse * max(dot(normal, to_light), 0), 0, 1)`.
The direction is normalized and colors/intensities are validated. Author base
colors as sRGB, decode before lighting, and encode once for sRGB display/capture.
Use an sRGB attachment's conversion or an explicit conversion for a non-sRGB
output, never both. Do not apply the 2D renderer's byte-space lighting/blending
convention to 3D.

Reuse the existing entity-based text UI for a small progress/completion overlay.
Compose it after the scene with depth disabled, accounting explicitly for its
byte-space color convention at the output boundary. Include that overlay in
captures. This does not select new widgets, typography or general UI layout.
Surface/device setup should be shared with 2D where useful. Public APIs may be
redesigned; migrate current callers and document material changes instead of
preserving obsolete interfaces. Keep the existing 2D visual references intact.

The validation targets are native Metal on the reference macOS machine and
actual browser WebGPU and WebGL2 paths. This is a target matrix, not a claim that
3D support has shipped or has been verified. Probe required color/depth/readback
formats and limits on each backend; explicitly report unavailable capability,
with no silent software 3D fallback. Existing 2D WebGL2 requires floating-point
color attachments; a shared overlay path must account for that requirement.
Keep adapter/device choice outside simulation. Resize, zero-size suspension,
surface loss and readback failures must have explicit host behavior.

### Evidence

Keep semantic and geometry/projection assertions GPU-independent. GPU tests must
exercise perspective size changes, occlusion independent of submission order,
winding, transformed normals, lighting and both sRGB/non-sRGB output handling.
Use interior probe regions for exact geometric expectations and declared color
and edge tolerances for images. Choose and record numeric tolerances with the
fixtures before accepting results; retain expected/actual/difference images and
backend details. Never infer portable GPU pixel equality from one adapter.
Native offscreen rendering, the actual native player and actual browser GPU
players each supply evidence; Node WASM execution alone proves no GPU behavior.
Capture state correspondence follows the [asynchronous capture
contract](inspection.md#asynchronous-capture-contract).
