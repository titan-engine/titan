# 0007. Web builds without emscripten

- Status: Accepted
- Date: 2026-10-13

## Context

Web is a nice-to-have (demos), but we want to keep it working from day one rather than port later. Most C
libraries assume emscripten for the web. A spike (2026-10-13) showed C code, including flecs, runs on plain wasm32
using wasi-libc for libc, with identical results to native and a 590 KB (180 KB brotli) flecs build.

## Decision

- Web builds use clang targeting `wasm32-wasip1` with wasi-libc linked statically (`cmake/wasm32.cmake`).
- The module imports nothing from WASI. `src/platform/web/wasi_shim.c` implements the few `__wasi_*` syscall
  wrappers libc needs on top of a tiny host interface (`env.titan_write`, `env.titan_now`, `env.titan_exit`),
  provided by our JS. `fopen` fails cleanly: there is no filesystem; assets go through the engine.
- Stack is 1 MiB with `--stack-first` so overflow traps instead of corrupting memory.
- Tests run as wasm in Node (`just test web`).
- Libraries that require emscripten are not used. If we need one, we build our own (maintainer's preference).

## Consequences

- No emscripten toolchain or its JS runtime; we own a small JS host that grows with the platform layer.
- No threads on web for now (wasm threads need shared memory and cross-origin isolation). Engine code must work
  single-threaded.
- When libc starts needing a new syscall, the link fails with a duplicate `__wasi_*` symbol; add the wrapper.
- Needs `wasi-libc` and `wasi-runtimes` (compiler-rt builtins for wasm). CI should pin their versions.
