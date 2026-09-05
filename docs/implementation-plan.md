# Implementation plan and quality gates

Pending execution work lives in [Titan Development](https://github.com/orgs/titan-engine/projects/1)
and linked [GitHub issues](https://github.com/titan-engine/titan/issues).
Use [the contributor workflow](workflow.md) for approval, ownership, dependencies,
PRs, stacks, reviews and autonomous integration. Proposed ideas are not approved
implementation. Do not duplicate the backlog here.

The accepted procedural RPG and standalone arena milestones, shared host tooling,
entity-based UI, snapshots/replay, quest journal and first loose-file PNG exercise
remain complete. See [handoff](handoff.md), [asset evidence](assets.md),
[journal](journal.md), [replay](replay.md), [UI](ui.md) and the
[v0.4.0 release](releases/v0.4.0.md). Historical execution plans remain in Git.
Engine package versions remain 0.4.0; no new release is selected.

The [vision](vision.md) and [design requirements](design-requirements.md) retain
firm commitments, tentative preferences and open questions. Backlog migration
does not change their certainty or approve all future capabilities. Use issues
for selection and execution, and update design docs when decisions change.

See [acceptance deadlines](acceptance-timeouts.md) for configurable build/runtime
limits, owned-process cleanup and CI evidence headroom.

## Constraints and quality gates

Preserve the accepted RPG behavior and software checksum `f7a298f62ad75c1c` for
changes unrelated to its visuals. Preserve arena initial `e096abf94fd12c24`
and winning replay `b5cf61da6f50efd7`, including existing no-dash survival
semantics. Keep discovery authentication, browser control opt-in, field
validation, deterministic safe points, and bounded diagnostics.
Do not silently present transport timeouts as cancellation of running systems.

The next objective does not yet schedule editor, 3D, networking, scene format,
broader asset pipeline or broad reflection redesign work. Issue #6 selects only
the bounded opt-in [executor slice](executor.md), retaining sequential defaults.
This is a scheduling boundary, not a revision of the agreed vision. Use the fixed
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
node --test web/inspector/*.test.mjs
node --test web/shared/*.test.mjs
node --test web/play/*.test.mjs
python3 scripts/test-rpg-replay.py # add --gpu on desktop
node scripts/test-rpg-replay.mjs
python3 scripts/test-rpg-assets.py # add --gpu on macOS
node scripts/test-rpg-assets.mjs
cargo check -p titan --lib --no-default-features
```

Preserve CI coverage for the starter and both games. Run
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
