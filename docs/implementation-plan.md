# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Shared replay through the RPG: selected

The user approved pushing the arena replay increment first, then adding RPG
snapshot-backed replay and extracting shared recording/playback machinery as both
games adopt it. Verify CI on both pushed revisions; keep coherent local commits
through implementation and push the completed increment. The first pushed
revision is `fb9b1d5`; its CI run is
[33950710706](https://github.com/titan-engine/titan/actions/runs/33950710706).

The RPG acceptance scenario crosses shard collection and shrine activation,
including a mid-quest snapshot origin. Keep gameplay snapshot contents and
validation game-owned. Extract reusable bounded recording, consumed input edge
encoding and playback progression into Titan, then migrate the arena. Preserve
arena historical recording compatibility and both games' reference pixels.
Validate both games headlessly and in native/browser playback, including pause,
step, restart, input isolation, bounded imports and end-of-recording verification.
Scrubbing, speed controls and difficulty settings remain deferred.

## Arena interactive replay: complete

Snapshot-backed recordings now play in the native and browser arena players,
with pause, single-step, restart playback, inspection, input isolation and a
fresh-live exit. Loading a mid-run save starts a valid new recording. Bounded
imports are verified before replacing the scene; playback preserves host time,
auto-pauses at its end and checks complete final state and pixels.

Native GPU, actual WASM, browser file controls and focused Rust/JavaScript checks
pass, including mid-dash/contact origins and historical v1 recordings. See
[interactive replay and evidence](arena-replay.md). Scrubbing and speed controls
remain deferred. The work is pushed on `main` at `fb9b1d5`; CI verification is
part of the current shared replay increment.

## Arena mid-run save/load: complete

The save/load work was pushed on `main` with the README preview fix at `a4ef146`.
The `v0.3.0` tag remains on the preceding UI revision. Mid-dash/contact round trips
preserve complete state and pixels, validate before installation, rebuild UI,
clear transient input and preserve host frame/inspection identity. The shared
tooling addition is bounded CLI `--arguments-file` support. Subsequent replay
work replaces recording invalidation with an embedded snapshot origin. See
[arena snapshots](arena-save-load.md) and [the persistence boundary](save-load.md).
No general ECS serializer or future save-format compatibility is promised.

## Design coverage and pending scope

The [vision](vision.md) and [design answer coverage](design-requirements.md)
retain the agreed product and architecture direction independently of milestone
completion. The coverage record accounts for both original planning answer
rounds, including tentative preferences and [unresolved questions](open-questions.md).
An unscheduled capability is not a discarded requirement.

The completed documentation audit restored that intent before the UI exercise.
Arena inspection now lists all entities, including pooled enemy activity and named
UI entities. Its HUD and the RPG quest label use shared ECS text components;
restart is an ECS button. Browser host controls and diagnostic panels remain
separate tooling.

Agreed capabilities that still need future implementation or broader coverage
include broader UI layout and typography; broader save/load coverage following
the [documented boundary](save-load.md); automatic parallel scheduling with
configurable determinism/throughput policy; replay scrubbing and speed controls;
the full generated/file-backed asset model and eventual native asset format;
and the longer-term rendering and multiplayer directions in the vision. The arena
exercises snapshots and interactive/headless replay; procedural demo assets do
not establish completion of the broader asset requirements. These remain
retained commitments.

## Entity-based UI: complete locally

The approved sequence is implemented and verified in local commits:

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

Agents can discover UI entities, inspect text and position, observe game-driven
updates and exercise the restart button. [UI verification](ui.md) records native
and browser pointer use, shared rendering, regressions and limitations. The
[save/load boundary](save-load.md) is documented and the subsequent arena snapshot
exercise above is complete locally. Difficulty settings remain a future possibility.

The UI source milestone `v0.3.0` is pushed at `6fc824d`, after
[CI run 33948720187](https://github.com/titan-engine/titan/actions/runs/33948720187)
passed that exact revision. Cargo package versions remain `0.1.0`, consistent
with the earlier source-milestone tags. Save/load commits are on `main` and excluded
from that tag; no crates were published.

## Input and live-player inspection: complete

The authorized input consolidation and actual-player inspection work is complete
in local commits. [Verification](live-player.md) records 23 passing checks,
independent reviews, real native/browser diagnosis and retained recordings.
The browser loss recording also replayed in a new native headless process with
identical state and pixels. That increment preserved the then-current RPG/arena
reference checksums; the later UI exercise intentionally changes the RPG label.

Public APIs now cover buffered buttons, shared browser input cancellation,
registered read-only queries and borrowed request policy. Arena owns its live
session, clock transitions and bounded consumed-input recording/replay. The
starter remains externally copyable. No broader application framework was needed.

The user accepted dash playability; its cooldown remains unchanged. Difficulty
settings are a possible future feature, not selected work. The next engine
objective was the completed UI exercise above. Remote CI remains a gate if these local commits are pushed.
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

Preserve the accepted RPG behavior and software checksum `f7a298f62ad75c1c` for
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
node --test games/arena/web/play/*.test.mjs
python3 scripts/test-macos-bundles.py # macOS
python3 games/arena/scripts/test-live-player.py # desktop GPU/window required
```
