# GPU sprite renderer

`titan-render-wgpu` renders the engine's `RenderFrame` and `ImageAssets` with real
textured GPU quads. Its render inputs have no dependency on an `App`, clock, or inspection
transport. `GpuRenderer` handles GPU drawing; `SurfaceRenderer` optionally owns
a default surface/device/queue for native and browser presentation. Native Metal/Vulkan/DX12/GL and browser WebGPU/WebGL2 use the
same wgpu 30 pipeline. WebGL2 needs floating-point color attachment support for
the RGBA16Float intermediate target.

```rust,ignore
let mut renderer = GpuRenderer::new(device.clone(), queue.clone(), surface_format);
renderer.prepare(&frame, &assets)?;
let mut encoder = device.create_command_encoder(&Default::default());
renderer.render(&mut encoder, &surface_texture_view)?;
queue.submit([encoder.finish()]);
```

For default surface setup, create a wgpu surface from your window/canvas and use:

```rust,ignore
let mut renderer = SurfaceRenderer::new(&instance, surface, width, height).await?;
let (width, height) = renderer.resize(width, height);
let presented = renderer.render(&frame, &assets)?;
```

This adapter requests portable limits, prefers non-sRGB output, bounds resize to
the texture limit, suspends zero-sized surfaces, and handles transient acquisition
failures. Lost/invalid surfaces return errors. It owns no game, clock, event loop,
or window/canvas setup. See [migration and lifecycle policy](../../docs/rendering.md#surface-adapter-migration).

When using `GpuRenderer` directly, the runner acquires the adapter/device,
configures and resizes its surface, submits commands, presents, and handles loss. `render` accepts a
single-sample color target with the constructor's format. Changing output size
requires no renderer reconfiguration: a second GPU pass scales the logical
framebuffer to the entire target with nearest-neighbor sampling. Use integer
output dimensions to preserve evenly sized pixels; aspect-ratio policy belongs
to the runner.

Sprites are ordered by layer, order, then insertion order. They support integer
pixel scale, clipping, nearest texture sampling, byte-rounded tint, and
source-over alpha. GPU blending uses a premultiplied RGBA16Float intermediate to
retain low-alpha color precision; the presentation pass converts to straight
alpha. For sRGB targets it additionally decodes the byte-space result before the
attachment's automatic encoding, matching the software renderer's byte-space
color convention. Fully transparent output is canonical transparent black.
Opaque scenes generally match exactly, while alpha composition can differ by a
small rounding amount from the integer CPU reference.

Each `prepare` validates dimensions/assets/buffer limits before replacing the
previous frame. Image uploads are deduplicated within that frame and refreshed
on subsequent frames; this intentionally avoids stale numeric ImageIds when
switching asset collections. Persistent asset caching and batching are future
optimizations. There is no CPU rasterization or framebuffer upload fallback.

Run hardware validation explicitly:

```sh
cargo test -p titan-render-wgpu --test offscreen -- --ignored --nocapture
```

This requires a working native GPU adapter and fails if none is available. It
checks textured quads, ordering, clipping, scaling, tint, low-alpha composition,
transparent backgrounds, both sRGB and unorm output, presentation resize, and
asset collection replacement against the software reference. A second hardware
test replays the full RPG reference route, verifies shrine completion, and
compares the extracted 160 × 112 GPU image with the exact software capture
(checksum `f7a298f62ad75c1c`) in both output color formats. Primary opaque
pixels and integer scaling must match exactly; other channel differences default
to at most 2/255. Set `TITAN_GPU_TOLERANCE=0` to request exact comparison or choose
another explicit u8 tolerance for a particular adapter. The engine's software
renderer remains the deterministic capture/reference implementation.

## Opaque 3D offscreen rendering

`GpuRenderer3d` consumes the validated `titan::render::three_d::RenderFrame3d`
snapshot. It owns one color texture and one `Depth24Plus` attachment, and no
surface, simulation, readback, or submission machinery:

```rust,ignore
use titan::render::three_d::BaseColor;
use titan_render_wgpu::{GpuRenderer3d, wgpu};
let mut renderer = GpuRenderer3d::new(
    device.clone(), 640, 480, wgpu::TextureFormat::Rgba8Unorm,
)?;
renderer.prepare(&frame, BaseColor::rgb(24, 32, 48))?;
let mut encoder = device.create_command_encoder(&Default::default());
renderer.render(&mut encoder)?;
// Copy renderer.color_texture() to a padded readback buffer here, or use
// renderer.color_view() for a later presentation/composition pass.
queue.submit([encoder.finish()]);
```

Meshes upload as indexed triangles with GPU projection × view × model position
transforms. Preparation applies inverse-transpose normal transforms using the
CPU data API's f64 intermediates, which also handles extreme finite scales.
The fragment shader normalizes interpolated world-space normals and applies
bounded ambient plus directional Lambert lighting in linear RGB. Counterclockwise
front faces, back-face culling, depth clear 1, strict less comparison and depth
writes preserve the CPU contract. Draws retain frame key order, so the lowest key
wins exact depth ties. Each render clears both attachments and uses one sample.

Allowed outputs are `Rgba8Unorm`, `Bgra8Unorm` and their sRGB variants. All store
sRGB-encoded RGB with opaque alpha: the sRGB attachment encodes linear shader
output, while the unorm shader explicitly encodes once. Clear colors use the
same policy. BGRA readback requires channel swizzling. These are format policies,
not a promise of backend availability: the host must probe adapter format usages
and report adapter/device/validation or readback failures. There is no software
fallback. The renderer requires only portable vertex/index/uniform facilities,
including one dynamic uniform binding; no storage buffers or base-vertex draw
feature is needed on WebGL2.

`resize(width, height)` rebuilds both attachments when dimensions change and
always invalidates prepared draws. Update the camera aspect in the next frame.
Zero dimensions return `InvalidDimensions`; the host should suspend drawing.
Failed resize retains the old target allocation/dimensions, with rendering
invalidated. Targets are bounded by device dimensions and `MAX_3D_TARGET_BYTES`
(64 MiB, charged at eight bytes per color/depth pixel). Uniform and geometry
uploads are checked against device buffer and addressing limits before GPU
allocation, in addition to the CPU frame budgets. Unsupported formats/limits and
unrepresentable transformed geometry produce `Gpu3dError`. Failed preparation
invalidates old draws; `render` returns `NotPrepared` until preparation succeeds.
Frame uploads are rebuilt without persistent mesh-handle caching, so replacement
and collection changes cannot silently reuse geometry. Native/browser adapter
acquisition, error scopes, device loss, surface presentation and readback remain
the host's responsibility.

## 3D players and UI composition

`SurfaceRenderer3d::new(instance, surface, width, height).await` uses the same
adapter/device acquisition, resize and presentation implementation as
`SurfaceRenderer`. `render(scene, clear, overlay, image_assets)` presents one
immutable `RenderFrame3d` plus the game-owned ECS-extracted `RenderFrame` UI.
The host owns keyboard/focus events, fixed ticks, camera aspect and extraction.
`adapter_info()` reports the actual backend. Existing 2D callers need no migration.

`resize` returns bounded dimensions (2048 per axis and the device limit for 3D),
which hosts must use for browser backing sizes and, by default, camera aspect.
For a fixed camera, call `set_aspect_ratio(Some((16, 9)))?` and use the same ratio
in the camera. Scene and UI targets fit inside the surface, centered with black
letterbox/pillarbox bars. In this mode oversized backing surfaces scale both axes
together to preserve window/canvas proportions, including high-DPI surfaces.
Fitting rounds down to whole pixels (at least one pixel
per axis). Both ratio terms must be nonzero. `set_aspect_ratio(None)` restores
full-surface presentation; offscreen capture dimensions are unaffected.
Zero dimensions
suspend presentation and preserve previous allocations; `suspended()` reports
this state and `size()` reports the retained nonzero target size. Timeout and
occlusion skip a frame; outdated surfaces reconfigure and skip; suboptimal
frames present then reconfigure. Lost/invalid surfaces and asynchronous device
errors become actionable `Err` messages. Hosts must stop or recreate the player
on these errors. Default acquisition probes required color/depth usages and
float attachment filtering/blending; unsupported WebGL2 configurations fail
explicitly. There is no software 3D fallback.

For offscreen composition use
`GpuSceneRenderer3d::new(device, queue, width, height, output_format)`,
`prepare(scene, clear, overlay, image_assets)` and `render(encoder, target_view)`.
The caller owns the output texture (including COPY_SRC when later reading it),
submission, device error scopes and readback. This is the same composition used
by the player; no game capture protocol is registered here. Output accepts RGBA8
or BGRA8 unorm/sRGB, single sample. Resize or failed preparation invalidates the
whole prepared composition; invalid resize preserves the old targets.

The scene uses the existing opaque color/depth pass. UI draws into a separate
transparent layer using the unchanged sprite renderer's byte-space blending
and nearest scaling. The fullscreen, depth-disabled composition pass decodes
both layers' encoded RGB, blends straight UI alpha over the scene in linear
light, then writes linear RGB to sRGB targets or explicitly encodes for unorm
output. Alpha stays opaque after composition. Thus an opaque UI color retains
its authored display bytes and a transparent pixel retains the scene; translucent
UI blends in linear light at this boundary. Overlapping UI sprites retain their
existing byte-space semantics. Use a transparent overlay clear for visible 3D.

Scene color/depth keeps its existing 64 MiB allocation budget; the separate UI
layer costs four more bytes per scene pixel, and the sprite intermediate costs
eight bytes per logical UI pixel (also limited to 64 MiB). Image uploads retain
existing sprite limits. The surface's 2048-axis bound keeps ordinary presentation
below these allocation budgets.

Explicit native GPU verification:

```sh
cargo test -p titan-render-wgpu --test composition -- --ignored --nocapture
```

This checks all four output formats, transparent/opaque/translucent UI against
independently calculated linear-light expectations (tolerance two bytes per
channel), nearest scaling after resize, zero-size rejection and stale-frame
invalidation. It retains JSON plus actual/expected/difference PNG triples in
the temporary directory `titan-composition-evidence`; override this with
`TITAN_COMPOSITION_EVIDENCE_DIR`. Verified on Apple M5 Pro Metal (24 cases).
Player-native and actual-browser evidence lives with the
collection-room hosts. The established 2D offscreen tests and exact RPG
reference remain unchanged.

## Owned asynchronous 3D capture

`OwnedGpuCapture::three_d(device, queue, frame, width, height, clear)` submits a
fresh immutable `RenderFrame3d`, including its resolved mesh assets, into its own
RGBA8 target. This low-level constructor captures only the scene; collection-room
scene/UI capture registration remains the integration work in #48.
`GpuSceneRenderer3d` above supplies the shared composition for that integration.
It never reads the presentation renderer's prepared cache.
`poll(elapsed)` is nonblocking and returns one `Image`, an error, or no result yet.
Use a monotonic host timer while paused, including in browsers; yield between
polls and do not depend on animation frames. Elapsed time begins at request
acceptance, before preparation. The staging allocation (including row padding)
is capped at 32 MiB; mapping has a five-second deadline. Frame geometry retains
its existing validated hard caps. No GPU handles or application borrows need
cross a wait. Drop/cancel aborts mapping and destroys staging storage. On cancellation or
timeout, call `job.retire(move || drop(completer))` to retain admission until
`Queue::on_submitted_work_done` confirms backend submission retirement. Keep
polling the device until that callback fires; dropping the producer early would
allow unbounded queued work through repeated canceled requests.

Hosts retain the inspection `CaptureCompleter` alongside the job, encode the
returned image with `titan_diagnostics::png_capture`, and complete it once. Keep
the producer alive through cleanup, and discard canceled/late work. The common
`PendingCapture` owns provenance, admission and the end-to-end deadline,
including PNG encoding. GPU errors use fixed bounded diagnostics. Unsupported
GPU hosts should not register a capture handler.

The shared native/browser fixture now also exercises the common asynchronous
dispatcher with a known accepted tick and PNG response, changes its source world
before readback, and tests frozen pixels, padded resize, cancellation, timeout
and subsequent capture after late callbacks. Its JSON includes capture response
identity and an inline PNG, without runtime discovery credentials.

Actual native mapping failure is covered by
`cargo test -p titan-render-wgpu --lib aborted_backend_map -- --ignored`.
It aborts a pending backend map, checks the bounded `Readback` error and verifies
queue retirement; it does not require manufacturing driver loss.
