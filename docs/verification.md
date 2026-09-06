# Verification and quality gates

Use this guide to select checks for the affected runtime or tooling. Start with
[CONTRIBUTING.md](../CONTRIBUTING.md) for contribution setup and
[workflow.md](workflow.md) for approval, ownership, review and integration policy.

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
owned asynchronous capture and collection-room player integration; they do not grant approval of implementation issues.
Issue status and prerequisites remain on the GitHub board. Existing platform
limitations remain in effect until the corresponding runtime evidence is added.

Each implementation increment must pass:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check -p titan -p titan-protocol -p titan-browser --target wasm32-unknown-unknown
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
cargo check -p titan --lib --no-default-features
```

Preserve CI coverage for the copied starter and all standalone games. Run
native GPU readback and inspect the browser canvas when rendering changes.
Software images are exact references; GPU comparisons are integration evidence.
Commit small coherent increments, keep current examples compiling, and document
material API migrations alongside the affected usage guide.

The core native RPG/arena acceptance harnesses retain bounded failure evidence;
see [local retrieval, CI artifacts and controlled-failure verification](acceptance-evidence.md).

Standalone and tooling gates:

```sh
python3 scripts/test-acceptance-evidence.py
python3 scripts/test-acceptance-failure-integration.py
python3 scripts/test-build-tools.py
python3 scripts/test-generated-assets.py
python3 scripts/test-starter.py --browser
cargo fmt --manifest-path games/arena/Cargo.toml --all --check
cargo test --manifest-path games/arena/Cargo.toml --all-targets
cargo clippy --manifest-path games/arena/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/arena/scripts/test-control.py
cargo check --manifest-path games/arena/Cargo.toml --lib --target wasm32-unknown-unknown
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
cargo test --manifest-path games/collection-room/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path games/collection-room/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/collection-room/scripts/test-control.py
python3 games/collection-room/scripts/build-browser.py
node games/collection-room/scripts/test-browser.mjs
node --test games/collection-room/web/play/*.test.mjs
python3 games/collection-room/scripts/test-player.py # desktop GPU/window required
cargo test -p titan-render-wgpu --test composition -- --ignored # native GPU
```

For actual browser GPU acceptance, serve the collection-room `web/` directory
and open `/play/test.html?backend=webgpu` and `?backend=webgl2`. Each must report
its own pass; Node execution is CPU/WASM evidence only.

Adventure cooperative room gates:

```sh
cargo fmt --manifest-path games/adventure/Cargo.toml --all --check
cargo test --manifest-path games/adventure/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path games/adventure/Cargo.toml --all-targets --all-features -- -D warnings
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
