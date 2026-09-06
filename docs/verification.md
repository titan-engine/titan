# Verification and quality gates

Use this guide to select checks for the affected runtime or tooling. Start with
[CONTRIBUTING.md](../CONTRIBUTING.md) for contribution setup and
[workflow.md](workflow.md) for intake, ownership, review and integration policy.

See [acceptance deadlines](acceptance-timeouts.md) for configurable build/runtime
limits, owned-process cleanup and CI evidence headroom.

## Constraints and quality gates

Preserve the accepted RPG behavior and software checksum `f7a298f62ad75c1c` for
changes unrelated to its visuals. Preserve arena initial `e096abf94fd12c24`
and winning replay `b5cf61da6f50efd7`, including existing no-dash survival
semantics. Keep the README preview at its committed 1280×896 nearest-neighbor
resolution; GitHub strips image-rendering CSS, so do not replace it with the
160×112 source capture. Keep discovery authentication, browser control opt-in, field
validation, deterministic safe points, and bounded diagnostics.
Do not silently present transport timeouts as cancellation of running systems.

The [3D rendering](rendering.md#3d-rendering-contract) and
[async capture](inspection.md#asynchronous-capture-contract) contracts distinguish implemented CPU 3D primitives, low-level GPU drawing and
owned asynchronous capture and collection-room player integration. Follow-up work
needs a concrete issue with scope and verification criteria.
Issue status and prerequisites remain on the GitHub board. Existing platform
limitations remain in effect until the corresponding runtime evidence is added.

Each implementation increment must pass:

```sh
cargo fmt --all --check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo check --locked -p titan -p titan-protocol -p titan-browser --target wasm32-unknown-unknown
```

For shared host, protocol, input, or game changes, also run the existing native
and actual-WASM control loops:

```sh
python3 scripts/test-control-loop.py
python3 scripts/build-browser.py
node scripts/test-browser.mjs
node --test web/inspector/*.test.mjs
node --test web/shared/*.test.mjs
node --test web/play/*.test.mjs
python3 scripts/test-rpg-replay.py # add --gpu on desktop
node scripts/test-rpg-replay.mjs
python3 scripts/test-rpg-assets.py # add --gpu on macOS
node scripts/test-rpg-assets.mjs
cargo check --locked -p titan --lib --no-default-features
```

Preserve CI coverage for the copied starter and all standalone games. Run
native GPU readback and inspect the browser canvas when rendering changes.
Software images are exact references; GPU comparisons are integration evidence.
Commit small coherent increments, keep current examples compiling, and document
material API migrations alongside the affected usage guide.

Follow the [evidence lifecycle](acceptance-evidence.md) for source retention,
ignored reruns and durable claims. The core native RPG/arena acceptance harnesses
retain bounded failure evidence;
see [local retrieval, CI artifacts and controlled-failure verification](acceptance-evidence.md).

Standalone and tooling gates:

```sh
python3 scripts/test-acceptance-evidence.py
python3 scripts/test-acceptance-failure-integration.py
python3 scripts/test-build-tools.py
python3 scripts/test-generated-assets.py
python3 scripts/test-starter.py --browser
cargo fmt --manifest-path games/arena/Cargo.toml --all --check
cargo test --locked --manifest-path games/arena/Cargo.toml --all-targets
cargo clippy --locked --manifest-path games/arena/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/arena/scripts/test-control.py
cargo check --locked --manifest-path games/arena/Cargo.toml --lib --target wasm32-unknown-unknown
python3 games/arena/scripts/build-browser.py
node games/arena/scripts/test-browser.mjs
node --test games/arena/web/inspector/bridge.test.mjs
node --test games/arena/web/play/*.test.mjs
python3 scripts/test-macos-bundles.py # macOS
python3 games/arena/scripts/test-live-player.py # desktop GPU/window required
```

Collection-room package and player gates:

```sh
cargo fmt --manifest-path games/collection-room/Cargo.toml --all --check
cargo test --locked --manifest-path games/collection-room/Cargo.toml --all-targets --all-features
cargo clippy --locked --manifest-path games/collection-room/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/collection-room/scripts/test-control.py
python3 games/collection-room/scripts/build-browser.py
node games/collection-room/scripts/test-browser.mjs
node --test games/collection-room/web/play/*.test.mjs
python3 games/collection-room/scripts/test-player.py # desktop GPU/window required
cargo test --locked -p titan-render-wgpu --test composition -- --ignored # native GPU
```

For actual browser GPU acceptance, serve the collection-room `web/` directory
and open `/play/test.html?backend=webgpu` and `?backend=webgl2`. Each must report
its own pass; Node execution is CPU/WASM evidence only.

Adventure cooperative room gates:

```sh
cargo fmt --manifest-path games/adventure/Cargo.toml --all --check
cargo test --locked --manifest-path games/adventure/Cargo.toml --all-targets --all-features
cargo clippy --locked --manifest-path games/adventure/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/adventure/scripts/test-control.py
python3 games/adventure/scripts/build-browser.py
node games/adventure/scripts/test-browser.mjs
node games/adventure/scripts/test-movement.mjs
node games/adventure/scripts/test-puzzle.mjs
node games/adventure/scripts/test-block.mjs
node games/adventure/scripts/test-sequence.mjs
node --test games/adventure/web/play/*.test.mjs
python3 games/adventure/scripts/test-player.py # desktop GPU/window required
```

Serve `games/adventure/web/` and open `/play/test.html?backend=webgpu`
and `?backend=webgl2` for actual browser GPU/control verification. The Node
WASM test compares the full state against a fresh native trace at every tick.

Factory construction package gates and player checks are documented in its [README](../games/factory/README.md#source-and-checks). Run them for factory changes alongside the workspace gates above.


## CI workloads and cache measurement

`.github/workflows/ci.yml` runs independent workloads on separate hosted runners.
Native and WASM each cover the workspace, copied starter and four standalone
games. macOS covers workspace GPU/RPG acceptance, copied bundles and the three
existing native game-player workloads. Matrix fail-fast is disabled so one
failure does not suppress evidence from the other workloads. The required
`Native checks`, `WebAssembly core check`, and `macOS development app bundles`
gates run even after dependency failure; each requires its entire matrix to
succeed. A failed, cancelled or unexpectedly skipped matrix cannot pass its gate.
There are no path filters or cache-hit conditions that bypass acceptance.

The command map below groups the original CI step names; their command bodies
remain in the workflow, including feature flags and explicit locked resolution.

| Workload | Existing coverage retained |
| --- | --- |
| Native workspace | Process timeout/cleanup, CI deadline, portable build policy, bounded failure evidence; formatting, procedural-only core, all workspace tests/examples, PNG corpus/fuzzing and failure upload; swarm, sparse-churn and mixed-schedule runners; generated assets; strict Clippy; structured CLI; RPG control, replay and assets |
| Native starter | `test-starter.py`: copy outside the checkout, initialize its lockfile deliberately, tests, Clippy and control |
| Native collection-room | Formatting, all-target/all-feature tests and Clippy, headless control |
| Native adventure | Formatting, all-target/all-feature tests and Clippy, control and playtest |
| Native arena and factory | Each game's formatting, all-target tests, all-feature Clippy and control; arena also runs real acceptance failure retention/cleanup |
| WASM workspace | Engine/protocol/browser target check; actual-WASM 3D primitives and browser GPU fixture build; browser adapter, control, replay/assets; inspector/shared/play JS unit tests |
| WASM starter | `test-starter.py --browser`: independent copied-project build and actual-WASM control |
| WASM collection-room | Browser build, actual-WASM acceptance and play JS tests |
| WASM adventure | Browser build, actual-WASM/native agreement, movement/puzzle/block/sequence checks and play JS tests |
| WASM factory | Browser build, actual-WASM construction, inspector/play JS tests |
| WASM arena | Target check, browser build, actual-WASM control, inspector bridge/play JS tests |
| macOS workspace | 3D/composition GPU readback and aborted-map checks, always-retained GPU evidence; RPG GPU replay/assets |
| macOS bundles | Build and execute relocated app bundles from external copied games |
| macOS adventure, factory, arena | Existing adventure player inspection/replay; factory ignored render test and construction player; arena live-player inspection/replay, bounded acceptance evidence, RPG/arena control and real failure retention/cleanup |

The original 45-minute shared shell-command deadline and 55-minute job bound
apply separately to each workload, preserving evidence/cleanup headroom. PR
supersession cancels only the same PR; main, manual and merge-group runs remain
independent. Sanitized failure uploads retain the same file allowlist and
seven-day lifetime, with shard-specific names. GPU evidence remains unconditional.

### Cache boundaries and refresh

`scripts/ci-cache.py` enumerates output profiles per platform and workload. Each
cache contains Cargo index/download archives and Git databases, root host outputs
needed by CLI/fixtures, and only that workload's game or copied-project outputs.
Native caches use `debug`; native workspace also retains the separate trybuild
host and target-triple `debug` outputs under `target/tests/trybuild` (excluding
its generated consumer project). WASM also includes host `release`, target-triple
`debug` and `release`, and the version-checked `titan/tools` bindgen installation.
The copied starter retains `target/starter-smoke`; relocated macOS bundle tests
retain `target/macos-bundle-smoke`. Neither is redirected into the source game's
build directory. Games keep their independent lockfiles and builds. This repeats
some shared dependencies across runners in exchange for concurrent execution.

Cache paths exclude runtime captures, discovery registrations, packaged apps,
Node output and downloaded registry source trees. Cargo re-extracts sources from
its archives. CI disables incremental compilation and debug symbols in dev/test
profiles to reduce retained artifact size; debug assertions and acceptance remain
enabled. Workloads still run every command after an exact cache hit.

Keys include cache schema, OS/architecture, native/WASM/macOS workload, full
compiler identity, runner image family, build-profile settings, all committed
Cargo manifest/lockfile hashes and UTC date. A dependency or toolchain change
starts cold. A new day restores only the same workload/graph prefix, then saves
one immutable daily generation after success. Later same-day runs restore that
generation without uploading again. Source changes still rebuild through Cargo;
a daily generation can therefore rebuild newer sources until the next refresh.
Do not widen fallback prefixes across toolchains or workloads to hide misses.
Changing path/profile policy requires a cache-schema bump. Daily generations
and old graphs can be evicted by GitHub; correctness never depends on retention.

Full main verification deliberately warms the default-branch scope for later
PR and merge-queue runs. Main pushes may instead reuse exact-SHA queue evidence;
cache-input changes, weekly scheduled runs and manual full runs retain warming
under the [main verification policy](workflow.md#exact-main-verification-and-cache-warming). PR caches belong to their merge ref and can be restored
by reruns of that PR, but cannot warm main or unrelated PRs. An exact inherited
main cache needs no redundant PR upload. Before the workflow first lands, its
new namespace has no main cache; pre-merge warm measurements prove only PR
rerun reuse. The same-day immutable policy bounds uploads, not the repository's
total cache storage. Inspect actual size and eviction behavior during measurement.
See GitHub's [cache matching and scope documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

### Measuring a change

Use completed attempts only; do not compare an incomplete or superseded run.
The read-only helper uses authenticated `gh` and writes compact JSON to stdout:

```sh
python3 scripts/test-ci-measurement.py
python3 scripts/measure-ci.py RUN_ID --attempt 1 > /tmp/ci-run.json
```

Record trigger, full source SHA, run/attempt URL, initial runner wait, per-job
start offset and duration, required-check wall time, cache restore/save step
time, restored archive bytes and keys, and total runner-minutes. Required-check
wall time runs from the attempt start through the last required gate completion;
runner-minutes sum job durations, including aggregate overhead. Start offsets
include dependency scheduling for aggregate jobs and are not pure runner queue
time. API timestamps have one-second resolution. Missing archive size is unknown,
not zero; cold save sizes can be inspected with `gh cache list --json
key,sizeInBytes,ref` and matched to the saved keys. Cache transfer sizes are
compressed archive sizes, not disk usage. Keep raw downloaded logs in ignored
or temporary storage, never commit them as timing evidence.

For a latency change, observe a genuinely cold workload-cache namespace, then
at least three completed representative warm runs with verified restore keys.
Rerunning the same PR attempt exercises its PR-scoped cache; it does not prove
cross-PR/default-branch reuse. Record this limitation and verify default-branch
warming after approved integration. Compare the median required-check time and
runner-minute tradeoff with a linked baseline, identifying the actual longest
job and transfer overhead. The five-minute warm median is an optimization target,
not a timing assertion. A miss needs a measured bottleneck and explicit maintainer
disposition before issue closure.

Exercise failure propagation on a disposable branch by making a necessary
workload fail before its ordinary checks. Confirm its required aggregate runs
and fails, and repeat with an upstream failure that skips a necessary job if
changing dependency layout. Restore the passing implementation and rerun required
PR checks. Preserve the full probe SHA, run URL and observed conclusions in the
PR evidence. Merge-queue and exact resulting-main verification happen only after
merge authorization; never weaken protections to test a rollout. Use the
[evidence lifecycle](acceptance-evidence.md) for retained measurements and reviews.
