# Titan

[![CI](https://github.com/titan-engine/titan/actions/workflows/ci.yml/badge.svg)](https://github.com/titan-engine/titan/actions/workflows/ci.yml)

An open source game engine designed to be as easy for AI agents to use as it is for humans.

- **C23**, built with LLVM clang. Clean builds take seconds.
- **[flecs](https://github.com/SanderMertens/flecs)** ECS with reflection, queries and a scene language.
- **WebGPU** rendering through the standard `webgpu.h` API.
- **Desktop and web.** Web builds use plain wasm32, no emscripten.
- **Scripting-first** games on a prebuilt engine, with native code as a first-class peer.
- **Agent-friendly by design:** text-first data, headless deterministic runs, and a live inspection protocol
  shared by agents and the editor.

Status: early development, nothing usable yet.

## Building

macOS (CI also builds and tests on Linux; Windows to follow):

```sh
brew install llvm lld cmake ninja just ccache wasi-libc wasi-runtimes node
just test          # native, with sanitizers
just test web      # wasm, tests run in Node
just check         # everything CI runs
```

## Documentation

- [AGENTS.md](AGENTS.md): conventions and workflow (for agents and humans alike)
- [docs/adr/](docs/adr/): architecture decisions

## License

[MIT](LICENSE). Vendored third-party code keeps its own license (flecs: MIT).
