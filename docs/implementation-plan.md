# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Current objective: consistent input and live-player inspection

Authorized on 2026-09-05 after the user accepted arena dash playability. Keep the
current cooldown; difficulty settings are a possible future game feature, not
part of this objective. Dash evidence lives in [arena-dash.md](arena-dash.md).

Execution order:

1. Reproduce and fix browser buffered-input cancellation across pause/focus loss.
   Consolidate demonstrated browser keyboard/pointer lifecycle and optional
   event-to-tick button accumulation. Keep bindings, buffering choices, RPG pulse
   cadence and game rules local. Preserve externally copied starter/game builds.
2. Attach inspection to the actual arena native/browser player. Pause at a fixed
   tick boundary, inspect entities and run state, and capture that same state.
   Keep inspection read-only by default and make remote control explicit.
3. Record consumed live input from restart, export a bounded reproducible run,
   and replay it headlessly to verify a suspicious contact, including dash edges.
   Report unsupported or incomplete recordings explicitly; do not claim replay
   fidelity when external mutations or missing history make it impossible.
4. Verify native/actual-WASM behavior, live host safe points, frame/revision
   correlation, controls, capture and replay. Run quality gates and independent
   review. Document APIs and a concrete playable diagnosis workflow.

Use subagents for implementation and verification; commit small coherent
increments. No broad application framework, rollback system, new game genre or
difficulty setting is implied. Local commits have not been pushed; inspect remote
CI if increments are subsequently pushed. No crate publication, visibility
changes or new tags are authorized. Engine crate versions remain 0.1.0.

## Completed host consolidation

Public surface presentation, input alias handling, browser session policy, PNG
capture and Cargo-resolved build tooling replaced demonstrated duplication.
Copied host setup fell from 1,066 to 739 lines per game (31%). Local quality,
native/WASM, GPU, relocated bundle and independent verification passed; no
build-speed gain is claimed. See [host setup audit](host-setup-audit.md).

The final remote CI gate passed for main commit `95a4061`:
[GitHub Actions run](https://github.com/titan-engine/titan/actions/runs/33944842147).
Host consolidation is complete.

## Constraints and quality gates

Preserve the accepted RPG behavior and software checksum `190a92085def5677` for
changes unrelated to its visuals. Preserve arena initial `e096abf94fd12c24`
and winning replay `b5cf61da6f50efd7`, including existing no-dash survival
semantics. Keep discovery authentication, browser control opt-in, field
validation, deterministic safe points, and bounded diagnostics.
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

Standalone and tooling gates:

```sh
python3 scripts/test-build-tools.py
python3 scripts/test-starter.py --browser
cargo fmt --manifest-path games/arena/Cargo.toml --all --check
cargo test --manifest-path games/arena/Cargo.toml --all-targets
cargo clippy --manifest-path games/arena/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/arena/scripts/test-control.py
cargo check --manifest-path games/arena/Cargo.toml --lib --target wasm32-unknown-unknown
python3 games/arena/scripts/build-browser.py
node games/arena/scripts/test-browser.mjs
node --test games/arena/web/inspector/bridge.test.mjs
node --test games/arena/web/play/input.test.mjs
python3 scripts/test-macos-bundles.py # macOS
```
