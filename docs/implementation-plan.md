# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Design coverage and pending scope

The [vision](vision.md) and [design answer coverage](design-requirements.md)
retain the agreed product and architecture direction independently of milestone
completion. The coverage record accounts for both original planning answer
rounds, including tentative preferences and [unresolved questions](open-questions.md).
An unscheduled capability is not a discarded requirement.

The completed documentation audit restored that intent before the entity-listing
work. The arena's browser inspection summary currently requests only the player;
enemies are entities, but are excluded by that request. Its HUD currently draws
text directly from game state. It does not yet fulfill the agreed direction that
in-game UI use the same entity/component model as the world. Browser host controls
and diagnostic panels are separate tooling; that requirement does not by itself
require converting them into game entities.

Agreed capabilities that still need future implementation or broader coverage
include entity-based in-game UI; early save/load and serialization design;
automatic parallel scheduling with configurable determinism/throughput policy;
interactive playback of input recordings; the full generated/file-backed asset
model and eventual native asset format; and the longer-term rendering and
multiplayer directions in the vision. Existing headless replay, snapshots and
procedural demo assets do not establish completion of those broader requirements.
These are retained commitments; the selected increment below exercises UI first.

## Entity-based UI: selected

The user approved this sequence, with frequent coherent commits:

1. Show all arena entities and expose pooled enemies' active state.
2. Convert the existing arena HUD to named UI entities with shared text and
   position components, preserving its appearance initially.
3. Make the in-game restart label an interactive button. Exercise logical pointer
   coordinates, hit testing and input consumption through headless, native and
   browser paths.
4. Reuse the components in the RPG for a compact quest-status display. Review
   before/after captures and retain gameplay assertions when updating its visual
   reference; this is an intentional visual change.
5. Document the save/load boundary between persistent game state and transient
   UI/host state, without implementing a general persistence system in this slice.

Agents must be able to discover UI entities, inspect text and position, observe
game-driven updates and exercise the restart button. Keep the first API grounded
in these two games rather than adding a speculative layout framework.
Record validation and material limitations in the UI guide. Recommend a version
or tag when this forms a useful verified release boundary; publication and tag
creation remain separate from that recommendation.

## Input and live-player inspection: complete

The authorized input consolidation and actual-player inspection work is complete
in local commits. [Verification](live-player.md) records 23 passing checks,
independent reviews, real native/browser diagnosis and retained recordings.
The browser loss recording also replayed in a new native headless process with
identical state and pixels. Existing RPG/arena reference checksums are unchanged.

Public APIs now cover buffered buttons, shared browser input cancellation,
registered read-only queries and borrowed request policy. Arena owns its live
session, clock transitions and bounded consumed-input recording/replay. The
starter remains externally copyable. No broader application framework was needed.

The user accepted dash playability; its cooldown remains unchanged. Difficulty
settings are a possible future feature, not selected work. The next engine
objective is the UI exercise above. Remote CI remains a gate if these local commits are pushed.
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
changes unrelated to its visuals. Preserve arena initial `e096abf94fd12c24`
and winning replay `b5cf61da6f50efd7`, including existing no-dash survival
semantics. Keep discovery authentication, browser control opt-in, field
validation, deterministic safe points, and bounded diagnostics.
Do not silently present transport timeouts as cancellation of running systems.

The next objective does not yet schedule editor, 3D, networking, scene format,
asset pipeline, parallel executor, or broad reflection redesign work. This is a
scheduling boundary, not a revision of the agreed vision. Use the fixed
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
node --test web/shared/input.test.mjs
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
python3 games/arena/scripts/test-live-player.py # desktop GPU/window required
```
