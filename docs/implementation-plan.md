# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Current objective: arena dash and measured iteration

The user authorized this objective on 2026-09-05. Add a short directional dash,
a fixed-tick cooldown and a visible ready indicator to the arena game. Use Space
in native and browser players and expose the same action through deterministic
input. Keep rules and presentation game-owned; change the engine only for a
demonstrated limitation.

Implementation is committed in `de60f84`. All 21 local gates passed, including
native/actual-WASM dash scenarios, unchanged no-dash survival semantics, GPU
readback and standalone/relocated bundle checks. Independent game and host
reviews found no actionable issues. Computer Use verified native Space input,
browser keyboard/pointer input, restart, cooldown and the browser winning replay.
The [dash verification report](arena-dash.md) records reviewed images and timings.

Cached game-source rebuild samples were 0.538s native and 1.227s browser;
inspection roundtrips were about 6ms and captures about 9ms. These are small-game
incremental samples, not clean-build or general productivity claims. No engine
API change was required.

Pending: user playtest and feedback on dash distance/cooldown. Tune only in
response to that feedback or a concrete defect. Commits are local; remote CI
must be inspected if these increments are subsequently pushed. No next engine
objective is selected.

Use subagents for implementation and verification to keep coordination compact.
No crate publication, visibility changes or new tags are authorized. Engine crate
versions remain 0.1.0.

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
changes unrelated to its visuals. Arena pre-dash visual baselines are initial `1e5d05f547d53435`
and winning replay `be61b1c710b101b6`; update them only after reviewing the
intentional dash HUD change, while preserving no-dash replay semantics. Keep discovery authentication, browser control
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
