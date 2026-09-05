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

The CPU data/math boundary is implemented in `titan::render::three_d` alongside
unchanged 2D APIs. `titan_render_wgpu::GpuRenderer3d` consumes these frames into
bounded offscreen color and depth targets. Collection-room players and UI composition use this renderer through shared
surface/device lifecycle. Asynchronous game capture integration remains agreed
design, **not yet an implemented collection-room capability**. Their execution scope lives in the linked issues of
[#42](https://github.com/titan-engine/titan/issues/42).

The [standalone collection room](../games/collection-room/README.md) now supplies
headless fixed-tick game rules, inspection/replay and extracted 3D frames using
this boundary. It also provides native and browser GPU players with a shared ECS overlay.
Image capture is not registered yet.

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
`Vec3`, `Quaternion`, `Mat4`, `Transform3d` and `PerspectiveCamera` implement these
operations without a new math dependency. Constructors normalize finite nonzero
quaternions/directions and reject invalid values or unrepresentable matrices.
`Mat4::columns` returns column-major f32 data; checked multiplication and
homogeneous vector transformation report overflow instead of returning infinity.
CPU intermediates use f64; this is not a portable bitwise floating-point promise.

`Mesh::new` owns finite positions, nonzero finite normals and u32 triangle
indices in immutable boxed slices (discarding spare input-vector capacity). It
rejects empty meshes, mismatched attributes, malformed/out-of-range indices and
zero-area triangles. Counts and byte sizes use checked arithmetic: at most
1,000,000 vertices, 3,000,000 indices and 64 MiB per mesh. Normals need not be unit
length; `Transform3d::transform_normal` applies inverse transpose and normalizes.
`Mesh::cube(size)` is centered at the origin with outward flat normals;
`Mesh::floor(size)` is an XZ square at Y=0 facing +Y. Both take positive side
lengths and use the same validating mesh constructor.

`MeshAssets::insert` returns a private, process-local `MeshHandle` scoped to that
collection and slot generation. `get` returns an `Arc<Mesh>`; `remove` invalidates
the handle, and `replace` returns a new handle that callers must store. Old
handles never resolve after replacement, slot reuse or collection reconstruction.
Previously retained meshes remain valid. Collections allow 65,536 slots, retire
exhausted generations, and panic rather than reuse an exhausted process identity.
Handles are not persistence IDs. There is no importer, asset graph or disk cache.

`RenderFrame3d::new` owns a validated camera, `Lighting3d` and resolved draws.
`Draw3d` supplies a handle, transform, opaque `BaseColor` and unique u64 `order`.
The frame sorts ascending by this key, making equal-depth submission deterministic
regardless of traversal order; duplicate keys are errors. For ECS entities, use
`(u64::from(entity.index()) << 32) | u64::from(entity.generation())` when no other
stable game-defined order is needed. No distance sort is imposed on opaque draws.
`BaseColor::rgb` is sRGB without alpha; `linear()` decodes it for lighting.
`Lighting3d::new(to_light, ambient, diffuse)` normalizes the direction and bounds
each intensity to 0..=1.

Each resolved `FrameDraw3d` retains the exact immutable mesh used at construction,
so asset replacement/removal cannot change an in-flight frame. Missing/stale
handles, duplicate order, invalid limits and matrix composition overflow return
`Frame3dError`, with no partial frame. `Frame3dLimits` permits lower budgets up to
the hard caps of 65,536 draws and 256 MiB of geometry. Geometry is charged once per
draw, including repeated handles, conservatively bounding retained/upload data;
allocator/Arc bookkeeping is excluded. Empty frames and zero budgets are valid.

Register a game-owned extractor receiving `&World`; `App` needs no changes and
owns no render/window/event-loop state. A fallible extractor stores
`Result<RenderFrame3d, Frame3dError>` as its snapshot, so the host can report asset
errors instead of rendering a stale successful frame:

```rust,ignore
use titan::{App, World};
use titan::render::three_d::*;

fn extract(world: &World) -> Result<RenderFrame3d, Frame3dError> {
    RenderFrame3d::new(
        *world.resource::<PerspectiveCamera>().unwrap(),
        *world.resource::<Lighting3d>().unwrap(),
        world.resource::<MeshAssets>().unwrap(),
        world.iter::<Object>().map(|(_, object)| object.0),
        Frame3dLimits::default(),
    )
}
// Object is a game component wrapping Draw3d. Insert camera, lighting, assets
// and objects before startup, then register the builder:
let mut app = App::new();
// ... populate app.world_mut() ...
app.add_extractor(extract);
app.update(); // completes startup and runs the first extraction
let frame = app.extracted::<Result<RenderFrame3d, Frame3dError>>().unwrap();
// After direct edits: app.refresh_extracted(); this does not advance simulation.
```

The complete [headless example](../examples/render_3d.rs) authors a cube and floor,
extracts them through `App`, and checks ownership and projection. Run it natively
or in actual WASM without a GPU:

```sh
cargo run --locked --example render_3d --no-default-features
node scripts/test-render-3d.mjs
```

Native unit tests cover validation, handle lifecycle, budgets, stable ordering,
geometry/winding, transform/normal and projection conventions. The WASM script
builds the same public example without default features and executes its exported
verification function in Node with a bounded subprocess. This is CPU evidence;
it does not claim browser GPU rendering. Existing 2D callers need no migration.

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

`SurfaceRenderer3d` shares device acquisition, bounded surface resize, suspension
and failure policy with the 2D `SurfaceRenderer`. It composes the existing
entity-based text UI after the scene with depth disabled. `GpuSceneRenderer3d`
provides the same composition into caller-owned offscreen targets for later
capture integration. The 2D overlay retains its byte-space rendering convention;
the compositor decodes its straight-alpha color to linear light, blends over
the decoded 3D scene, and encodes exactly once for the output format. This does not select new widgets, typography or general UI layout.
Surface/device setup should be shared with 2D where useful. Public APIs may be
redesigned; migrate current callers and document material changes instead of
preserving obsolete interfaces. Keep the existing 2D visual references intact.

The validation targets are native Metal on the reference macOS machine and
actual browser WebGPU and WebGL2 paths. The low-level GPU verification fixture below exercises this matrix separately
from game-player integration. Probe required color/depth/readback
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


### Low-level GPU verification

`GpuRenderer3d` owns GPU resources, with no `App`, surface, event loop or capture
protocol. See [its API and lifecycle](../crates/titan-render-wgpu/README.md) for
construction, preparation, resize, encoding and offscreen texture access.
Existing 2D callers need no migration. The 3D path uses a separate shader and
linear-light color policy; the sprite path and exact software references remain
unchanged.

Run the shared GPU fixture natively and in an actual browser:

```sh
cargo test -p titan-render-wgpu --test three_d -- --ignored --nocapture
python3 scripts/build-render-3d-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory web
# Open /render-3d/?backend=webgpu, then /render-3d/?backend=webgl2.
```

Each browser URL requests exactly one backend; unavailable adapters are failures,
not fallback passes. The page runs actual WASM vertex/fragment shaders and GPU
readback, displays the resulting images and offers full JSON evidence for
download. Browser execution has a 60-second deadline. A timeout requires reload
to discard that test session; it does not claim cancellation of GPU work.

The fixtures declare interior probe regions and color tolerances before
comparison. Expected images mark only asserted regions; unasserted edges do not
establish portable rasterization equality. Native artifacts and browser downloads
retain actual, expected and difference pixels plus adapter and comparison details.
CI runs native readback on macOS, uploads its JSON/PNG triples for seven days,
and compiles the actual browser fixture. Browser GPU execution remains an
explicit local verification step. Fixture hosts probe the required format usages
and collect wgpu validation errors before accepting evidence.

The fixture has been verified on Apple M5 Pro native Metal, browser WebGPU and
browser WebGL2 (Chromium ANGLE Metal). Each backend passed 36 image cases with
50 interior probes at a declared per-channel tolerance of 2. These results cover
the low-level offscreen renderer, not a native/browser 3D game player. Native
artifacts default to the system temporary directory's `titan-3d-evidence` folder;
set `TITAN_3D_EVIDENCE_DIR` to retain them elsewhere.

### Collection-room players

See the [game README](../games/collection-room/README.md) for native/browser
launch commands, controls, interactive recording playback and opt-in inspection.
The CPU-only game and controller remain available without the `player` feature.
The fixed 320 × 180 ECS overlay is scaled with nearest sampling over the scene;
window/canvas dimensions are bounded to 2048 pixels per axis. Zero sizes suspend
presentation until a nonzero resize. Outdated surfaces are reconfigured and
retried on a later frame, timeouts/occlusion skip presentation, and unrecoverable
surface/device errors stop the host with a diagnostic. No software 3D fallback
is selected. Native OS suspension drops and recreates the surface on resume.
