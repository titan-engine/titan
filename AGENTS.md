# Titan: guide for agents

Titan is an open source game engine in C23, designed to be equally usable by humans and AI agents. It is built
on flecs (ECS), renders through `webgpu.h`, runs on desktop and on the web without emscripten, and runs games
from scripts on a prebuilt engine. Decisions and their reasons are in `docs/adr/`. Read the ADRs relevant to
your task before starting.

## Commands

```sh
just test            # build + test with ASan/UBSan/LSan (preset: dev)
just test tsan       # ThreadSanitizer
just test web        # wasm32, tests run in Node
just fuzz <name> 60  # run a fuzz target for 60s (targets: fuzz/fuzz_<name>.c)
just fmt             # format; just fmt-check verifies
just tidy            # clang-tidy
just lint            # project rules (tools/lint.sh)
just check           # everything CI runs: run before every PR
```

Requires Homebrew `llvm lld cmake ninja just wasi-libc wasi-runtimes node` (and `ccache`, optional).
Apple clang is not supported (ADR 0002).

## Layout

```
include/titan/   public headers
src/core/        basic types, allocators
src/platform/    per-OS platform layer (web/: wasm host shim)
tests/           one executable per test_<name>.c, harness in tests/test.h
fuzz/            libFuzzer targets, seed corpora in fuzz/corpus/<name>/
third_party/     vendored code (flecs); maintainer approval needed to change
cmake/           toolchains and compiler defaults
tools/           lint script, wasm runner
docs/adr/        architecture decision records
```

## Workflow rules

- Small PRs, one concern each. The description says what changed, why, and how you tested it.
- `just check` passes before you open a PR. Never weaken a warning, sanitizer, lint or tidy rule to make
  code pass. Fix the code, or explain in the PR why the rule is wrong.
- Ask the maintainer before: adding any dependency, touching `third_party/`, `cmake/`, `CMakePresets.json`,
  `justfile`, `tools/`, CI, or changing public API shape. Disagree with an ADR by proposing a new one.
- Every bug fix includes a regression test (or a fuzz corpus entry for parser bugs).
- Work in your own worktree with its own `build/`. Never share build directories between worktrees.

## Code conventions

**Naming.** Public symbols start with `titan_` (functions, types) or `TITAN_` (macros, constants). snake_case.
Types have no `_t` suffix: `typedef struct titan_arena titan_arena;`. One module per header in `include/titan/`.

**Ownership is in the name.**
- `titan_x_create()` returns an object the caller owns; free it with `titan_x_destroy()` (which accepts nullptr).
- `titan_x_init(x*)` / `titan_x_fini(x*)` work on caller-provided storage.
- Anything else returns borrowed memory. Document its lifetime in the header if it is not obvious.

**Memory.**
- Allocate from arenas (`titan_arena`) by default. Raw `malloc`/`free` is only allowed in allocator modules and
  platform code (enforced by `tools/lint.sh`).
- Refer to things across systems by handle (flecs entity, generational index), not pointer.
- No VLAs. Size arithmetic is overflow-checked: use `titan_arena_alloc_array` or check before multiplying.

**Strings.** APIs take and return `titan_str` (pointer + length). NUL-terminated strings appear only at
boundaries with C libraries. Banned: `strcpy strcat sprintf vsprintf gets strtok strncpy strncat atoi atol atof`.

**Errors.** Recoverable failures return a status, `false` or `nullptr`, and callers check them
(`[[nodiscard]]` on functions whose result must not be ignored). Asserts are for programmer errors only, never
for bad input data.

**Integers.** `size_t` for sizes and counts, fixed-width types for data formats. With `-Wconversion` on, write
explicit casts and make sure they cannot truncate.

**Web constraints.** Code must work single-threaded and without a filesystem (no `fopen` on web). The stack is
1 MiB; put large buffers in arenas, not on the stack.

**Tests.** `REQUIRE(...)` before dereferencing anything a test allocated; `CHECK(...)` for the rest. Tests must
be fast and deterministic.

**Comments** explain why, not what. Keep them current.

## Third-party code

Vendored under `third_party/<name>/` with its license and a README stating the version and any local patches.
Built with warnings off but with sanitizers on. Upstream fixes go upstream.
