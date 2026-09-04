# GPU sprite renderer

`titan-render-wgpu` renders the engine's `RenderFrame` and `ImageAssets` with real
textured GPU quads. It has no dependency on an `App`, window, surface, clock, or
inspection transport. Native Metal/Vulkan/DX12/GL and browser WebGPU/WebGL2 use the
same wgpu 30 pipeline. WebGL2 needs floating-point color attachment support for
the RGBA16Float intermediate target.

```rust,ignore
let mut renderer = GpuRenderer::new(device.clone(), queue.clone(), surface_format);
renderer.prepare(&frame, &assets)?;
let mut encoder = device.create_command_encoder(&Default::default());
renderer.render(&mut encoder, &surface_texture_view)?;
queue.submit([encoder.finish()]);
```

The runner acquires the adapter/device, configures and resizes its surface,
submits commands, presents, and handles device/surface loss. `render` accepts a
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
(checksum `98618cd721c5b52d`) in both output color formats. Primary opaque
pixels and integer scaling must match exactly; other channel differences default
to at most 2/255. Set `TITAN_GPU_TOLERANCE=0` to request exact comparison or choose
another explicit u8 tolerance for a particular adapter. The engine's software
renderer remains the deterministic capture/reference implementation.
