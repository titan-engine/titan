# 0003. flecs as the ECS

- Status: Accepted
- Date: 2026-10-13

## Context

Scripting-first games (ADR 0009) need a dynamic ECS: components defined at runtime, with reflection so the
editor, scripts, remote protocol and serialization can all inspect them. flecs provides that, plus queries,
relationships, observers, a scripting/scene language, JSON and a REST explorer. It compiles in about a second
and adds little to binary size.

A spike (2026-10-13) confirmed flecs v4.1.6 builds and runs on wasm32 without emscripten, with identical results
to native (see ADR 0007).

## Decision

- flecs is vendored as its amalgamated `flecs.c`/`flecs.h` in `third_party/flecs/`, built as `titan_flecs`.
- Titan code may use the flecs C API directly. Wrappers are added where they make an API safer or more
  consistent, not by default.
- On web, flecs is built with an explicit addon list and without its own OS layer (`FLECS_NO_OS_API_IMPL`);
  Titan's platform layer supplies OS functions.
- Upstream fixes go upstream. Local patches are a last resort and must be listed in `third_party/flecs/README.md`.

## Consequences

- We get a mature ECS, reflection and serialization without writing them.
- flecs's design (entities as handles, systems over component arrays) shapes the engine. That is intended.
- Upgrading flecs is a deliberate step: replace the two files, run `just check`, note the version.
- Candidate upstream patch: recognize plain wasm32 as a target (we currently pass `ECS_TARGET_POSIX` and a stub
  `execinfo.h`).
