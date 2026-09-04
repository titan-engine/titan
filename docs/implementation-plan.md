# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Pending: choose the next objective

No new implementation objective has been selected. Duplicated host adapters
across standalone games are one demonstrated cost worth considering for small
consolidation. Choose the next objective with the user before starting it.
Keep current platform and separate player/inspection-instance limitations
visible in that decision.

## Constraints and quality gates

Preserve the accepted RPG behavior and software checksum `190a92085def5677` for
changes unrelated to its visuals. Keep discovery authentication, browser control
opt-in, field validation, deterministic safe points, and bounded diagnostics.
Do not silently present transport timeouts as cancellation of running systems.

No speculative editor, 3D, networking, scene format, asset pipeline, parallel
executor, or broad reflection redesign is preapproved for the next objective. Use the fixed
arena view unless the game demonstrates a camera requirement. Keep current
platform limitations documented rather than expanding platform scope.

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
node --test web/inspector/bridge.test.mjs
```

Preserve CI coverage for the starter and both games. Run
native GPU readback and inspect the browser canvas when rendering changes.
Software images are exact references; GPU comparisons are integration evidence.
Commit small coherent increments, keep current examples compiling, and document
material API migrations alongside the affected usage guide.
