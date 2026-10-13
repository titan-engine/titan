# 0002. C23, LLVM clang only

- Status: Accepted
- Date: 2026-10-13

## Context

An earlier attempt used generic-heavy Rust (Bevy-style ECS, wgpu). It worked, but compile times, multi-GB build
directories per worktree, and slow CI made iteration painful. The maintainer prefers reading C, and the engine's
core (flecs) is C. Rust would catch more bugs at compile time; we accept doing that work with tooling instead
(ADR 0005).

## Decision

- Titan is written in C23 (`C_STANDARD 23`, no GNU extensions in our code).
- The only supported compiler is LLVM clang >= 21 on every platform: Homebrew LLVM on macOS, clang on Linux,
  clang/clang-cl on Windows. Apple clang is rejected at configure time because it lacks LeakSanitizer and
  libFuzzer. CI pins one major version so warnings are identical everywhere.
- Being clang-only, we may use clang features freely: C23 (`nullptr`, `constexpr`, `typeof`, `[[nodiscard]]`,
  `#embed`, `bool`), `#pragma once`, `__attribute__`s, builtins.

## Consequences

- Builds take seconds and produce tens of MB, not GB.
- No MSVC support. Windows uses clang-cl, which is well supported.
- Memory safety is our responsibility: ADR 0005 is mandatory, not optional.
