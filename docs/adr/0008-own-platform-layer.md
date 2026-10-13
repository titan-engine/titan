# 0008. Our own platform layer

- Status: Accepted
- Date: 2026-10-13

## Context

The engine needs windows, input, audio, files, time and threads per OS. SDL3 is excellent but requires emscripten
on the web (ADR 0007). sokol_app shows a small C platform layer is practical.

## Decision

- Titan has its own platform layer in `src/platform/<os>/` behind a small, engine-shaped API.
- Order: macOS and web first, then Windows and Linux.
- Platform code is one of the few places allowed raw OS and libc calls (including malloc) outside allocators.
- The flecs OS API is implemented on top of it.

## Consequences

- Full control and no emscripten, at the cost of writing and maintaining per-OS code.
- Platform APIs must be designed for headless use too (ADR 0011): every capability has a headless implementation
  for tests and agents.
