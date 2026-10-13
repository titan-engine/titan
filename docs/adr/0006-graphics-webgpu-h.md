# 0006. Graphics through the standard webgpu.h API

- Status: Accepted
- Date: 2026-10-13

## Context

We need 2D and 3D rendering on desktop and web from C. `webgpu.h` (webgpu-native/webgpu-headers) is a standard C
API implemented by wgpu-native and Dawn, and maps directly onto WebGPU in browsers. wgpu-native ships prebuilt
binaries for macOS, Linux and Windows (v29.0.1.1 as of 2026-10), so native builds compile no graphics backend.

## Decision

- The renderer is written against `webgpu.h`.
- Native: link a prebuilt implementation (wgpu-native first; Dawn possible later).
- Web: implement `webgpu.h` as a thin bridge to the browser's WebGPU in our own JS host (no emscripten).
- Shaders are WGSL everywhere, so no shader cross-compilation step.

## Future direction (not planned work)

The maintainer may one day write Titan's own native implementation of `webgpu.h` with extensions for modern
features such as hardware ray tracing. `webgpu.h` is designed for this:

- New functions, objects, enums and structs use an implementation prefix (`wgpuTitanX`, `WGPUTitanX`) and are
  detected at runtime through `wgpuGetProcAddress` and feature queries.
- New enum values and chained-struct `sType`s use a registered enum block (e.g. wgpu-native has `0x0003_xxxx`).
  Titan would register its own prefix and block in the webgpu-headers registry.
- Chained structs (`nextInChain`) extend existing descriptors. Ray tracing additionally needs new objects
  (acceleration structures), new functions and a WGSL extension.

## Consequences

- One rendering API for all platforms; no shader translation step.
- We depend on webgpu.h evolution and on implementation-specific extensions for anything beyond core WebGPU.
- The renderer must stay within what browsers support unless a feature is explicitly native-only and optional.
