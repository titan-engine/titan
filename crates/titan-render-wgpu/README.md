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
