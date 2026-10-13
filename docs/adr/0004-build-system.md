# 0004. CMake + Ninja + just

- Status: Accepted
- Date: 2026-10-13

## Context

We need multiple build configurations (sanitizers, fuzzing, release, web), `compile_commands.json` for clangd and
clang-tidy, and one obvious command for everything. Agents work in many git worktrees at once, and the previous
project filled the disk with build output.

## Decision

- CMake (>= 3.28) with Ninja, configured only through `CMakePresets.json`: `dev` (ASan+UBSan, default), `tsan`,
  `msan` (Linux), `release`, `fuzz`, `web`.
- `just` is the entry point: `just build`, `just test [preset]`, `just fuzz`, `just fmt`, `just tidy`,
  `just lint`, `just check` (everything CI runs).
- Build output lives in `build/<preset>/` inside each worktree. Deleting a worktree deletes its builds.
- Each module compiles as a handful of translation units; flecs is one. ccache is used when installed.

## Consequences

- One worktree's builds take tens of MB, so worktrees are cheap. Never share a build directory between
  worktrees.
- CMake is verbose, but agents and editors understand it well. Keep CMake logic simple and in `cmake/`.
