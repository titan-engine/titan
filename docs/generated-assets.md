# Generated image cache fixture

Issue [#9](https://github.com/titan-engine/titan/issues/9) exercises one
deterministic generated PNG in `fixtures/generated-asset`. The fixture owns the
generator, its inputs/version, cache files and lazy lifetime. Titan continues to
consume the result through its existing RGBA `Image` boundary. This is a native
build/runtime tooling exercise; no engine asset registry, RPG integration, hot
reload, renderer change or additional format is introduced.

## Verification

The fixture is a workspace member, so the ordinary workspace formatting, tests
and Clippy gates cover it. `python3 scripts/test-generated-assets.py` also drives
separate processes to verify persistent cache reuse and recovery. The required
native CI job runs this process-level exercise alongside existing game coverage.

## Ownership and storage

`fixtures/generated-asset/src/generator.rs` is shared by the fixture's `build.rs`
and native runner. Its fixed 16×16 RGBA tile is controlled by a `u32` seed;
the explicit generator version covers its algorithm, dimensions, encoding and
cache envelope. Bump that version when those choices change. The cache filename
encodes both numeric inputs directly, avoiding hash collisions in asset identity.
Old versions remain separate files until the caller removes its cache directory.

Build output and the build cache live under Cargo's fixture `OUT_DIR`:
`generated.png` and `generated-cache/`. Cargo owns their lifetime. The runtime
requires `--cache-dir DIR`; the caller owns that directory and can delete it to
start cold. No user-wide cache or discovery location is used. `LazyTile` owns a
resident result after its first successful access. Construction does no I/O or
generation; subsequent access to that object uses memory. Eager access at startup
uses the same loader.

Each entry contains its seed/version and an integrity checksum followed by PNG
bytes. Reads are capped at an 8 KiB PNG plus the 24-byte envelope, and PNG decoding
checks dimensions, color/depth and a bounded allocation budget. Missing entries
generate; malformed entries regenerate; other I/O failures remain errors so an
unwritable cache is visible. The checksum detects accidental corruption; it is
not authentication for hostile writers. Same-directory temporary files and
rename prevent readers from observing a partially written entry. Concurrent
misses may duplicate deterministic generation. The fixture does not promise
single-flight generation, cache eviction or crash-durable directory metadata.

## Run and interpret

```sh
cargo run -p titan-generated-asset -- --cache-dir target/generated-asset-cache
cargo run -p titan-generated-asset -- --cache-dir target/generated-asset-cache
cargo run -p titan-generated-asset -- --cache-dir target/generated-asset-cache --seed 99
cargo run -p titan-generated-asset -- --cache-dir target/generated-asset-cache --generator-version 2
cargo test -p titan-generated-asset
python3 scripts/test-generated-assets.py
```

The runner emits JSON schema version 1 and fails on parity errors. `cache_outcome`
is `generated`, `reused` or `recovered`; `generation_count` counts only the lazy
cache path, while the uncached comparison has a separate counter. A warm disk
load decodes and validates the cache but does not call the generator. The startup
probe follows the lazy probe, so it is warm. The build-time fields report the
last actual `build.rs` execution, not the current runtime. The process harness
changes `TITAN_ASSET_BUILD_CHECK` twice to rerun the build script and verify its
warm cache; ordinary unchanged Cargo builds may skip the script altogether.

Changing the seed changes pixels. `--generator-version` demonstrates namespace
invalidation without changing the algorithm itself, so version-only variants
retain pixels. Tests verify lazy construction, memory-only second access,
reproducible PNG and decoded pixels, separate cache identities, malformed and
oversized entries, concurrent writers and repair after I/O failure. The runner
also writes and reads a loose PNG before decoding through Titan. Build-time
parity uses the build's default inputs even when runtime inputs are overridden.

A local debug-build run on 2026-09-05 (macOS arm64, Rust 1.98.1) measured:

| Operation | Cold process (µs) | Warm process (µs) |
| --- | ---: | ---: |
| Uncached PNG generation | 629 | 402 |
| First lazy access | 7,270 | 188 |
| Second lazy access | 0 | 0 |
| Subsequent eager warm load | 234 | 109 |

Both processes produced the same 195-byte PNG and RGBA checksum
`65c6af5c946efc09`. Cold/warm cache generation counts were 1/0, and the forced
warm build-script execution also generated zero times. These are single
`Instant` samples in microseconds, not a benchmark or speed guarantee. Zero
means below the reported microsecond resolution. First access includes cache
I/O, validation or generation, publishing on a miss, and the runner's PNG clone;
uncached timing includes generation and PNG encoding. Timings exclude compilation,
process startup and later parity probes. Disk sync makes a cold cache more
expensive for this tiny asset; reuse is demonstrated by counters and exact output,
not a performance threshold. The fixture has no browser filesystem path and
requires no GPU; existing native/browser game CI remains unchanged.
