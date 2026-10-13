# 0005. Sanitizers, fuzzing, static analysis and conventions

- Status: Accepted
- Date: 2026-10-13

## Context

In C, the mistakes agents typically make (missing frees, unclear ownership, off-by-one, overflow, races) compile
cleanly and often pass tests, then surface as crashes far from the cause. Rust would catch many at compile time.
We choose to catch them with tooling and conventions so the maintainer's review time goes to design, not memory bugs.

## Decision

Every PR must pass `just check`:

| Layer | What |
|-------|------|
| Warnings | `-Wall -Wextra -Wpedantic -Werror` plus `-Wshadow -Wconversion -Wsign-conversion -Wvla -Wimplicit-fallthrough -Wmissing-prototypes` and more (`cmake/titan_defaults.cmake`) |
| Sanitizers | Tests run under AddressSanitizer + UndefinedBehaviorSanitizer + LeakSanitizer (`dev`) and ThreadSanitizer (`tsan`). Linux CI adds MemorySanitizer (`msan`). Third-party code is instrumented too. |
| Fuzzing | Every parser of external input (scene files, assets, protocol messages, script bindings) has a libFuzzer target in `fuzz/` with a seed corpus. CI runs each briefly; longer runs happen on a schedule. |
| Static analysis | clang-tidy (`bugprone`, `cert`, `clang-analyzer`, `misc`, `performance`, `portability`), warnings as errors |
| Formatting | clang-format, checked in CI |
| Project lint | `tools/lint.sh`: bans unbounded string functions, and raw `malloc`/`free` outside allocator and platform modules |
| Web | The test suite also runs as wasm in Node (`just test web`) |

Conventions that make bugs less likely are listed in `AGENTS.md`: arenas by default, handles over pointers across
systems, length-carrying strings, ownership in function names, overflow-checked size arithmetic, REQUIRE before
dereferencing in tests.

## Consequences

- Sanitizer builds are slower to run; tests must stay fast.
- A bug fix comes with a regression test, or a fuzz corpus entry for parser bugs.
- Rules that tools cannot check yet are enforced in review. When a rule keeps being broken, automate it.
