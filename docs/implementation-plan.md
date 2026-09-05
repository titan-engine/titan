# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Release v0.4.0

The user authorized the next source tag and Cargo version alignment. All eight
engine workspace packages are now `0.4.0`; standalone arena/starter packages
remain `0.1.0`. All three tracked lockfiles reflect the local engine versions,
without third-party dependency updates. Publishing remains disabled, and protocol,
save and recording format versions are unchanged.

The release commit `cebefc3` passed all three jobs in
[CI run 33952161952](https://github.com/titan-engine/titan/actions/runs/33952161952).
At release completion, remote `main` and the pushed annotated `v0.4.0` tag
resolved to that revision.
See [release notes](releases/v0.4.0.md) and [next-task context](handoff.md).
The subsequent journal and file-backed sprite exercises are documented below.

## RPG quest journal: complete

The approved exercise adds a closed-by-default journal for the existing shard
and shrine objectives. Keyboard/pointer navigation, wrapped detail text and a
Close button use shared column layout, bounded bitmap text, scoped focus and
filtered UI extraction. Game-owned modal policy restores prior pause state,
clears stale input and preserves completed-playback isolation.

Live capture shows the journal; gameplay recording verification uses a canonical
closed-journal render including the existing HUD. Native/headless/actual-WASM
checks cover open-panel export, fresh playback, snapshot resets and inspection.
Physical native/browser interaction and exact GPU/software open/closed comparison
passed. Reference checksums and the crisp README preview are unchanged.
See [journal behavior and evidence](journal.md) and [UI APIs](ui.md).

The workspace, both games, external starter and macOS bundle gates pass locally.
GitHub Actions records CI for each pushed revision; verify the exact revision
when continuing. No release
or package version bump is part of this exercise.

## File-backed RPG player sprite: complete locally

The approved exercise loads the exact existing player sprite from a loose PNG in
native, browser and headless hosts. A default-enabled optional engine decoder
converts bounded static PNG data into the same `Image` used by procedural art.
Hosts own startup readiness, path/fetch failures and explicit retry. Shared build
tooling delivers browser resources and relocatable macOS bundle resources.
Replacing the file and restarting/reloading changes pixels without recompilation.

Restart, snapshots and playback retain the startup image; fresh verifiers load
matching art and reject a different final checksum. Native/browser/headless
acceptance covers replacements and failures. The committed PNG reproduces the
reference exactly, including GPU/software open and closed journal comparison.
See [asset boundaries, commands and evidence](assets.md).

The next feature is not selected. Hot reload, asset dependency/identity management,
generation caching, other formats and the eventual native format remain pending.
Consult the [asset vision](vision.md#rendering-and-assets) and requirements
R2.57–62 before extending this slice. Parallel scheduling needs a representative
workload and measurements; replay scrubbing/speed controls remain deferred.
Keep frequent local commits, complete review and local gates, then push a batch
and verify that exact revision in CI. No tag or package bump is part of this work.

## Shared replay through the RPG: complete

Both games now use Titan's bounded snapshot recording, consumed digital input
codec and playback cursor. Snapshot contents/validation, simulation and session
policy stay game-owned. The RPG restores despawned shards and inactive shrine
state from a mid-quest snapshot, then reaches the same complete quest and pixels
through native headless, native GPU and actual browser playback. The arena's
v1/v2 recordings and reference results remain valid after migration.

Workspace/arena Rust gates, actual native/WASM control loops, browser file controls,
external starter and macOS bundle checks pass. See [shared replay](replay.md),
[RPG controls and evidence](rpg-replay.md) and [local checks](replay/local-checks.json).
The user requested an initial push, implementation, then a second push with CI
verified for both revisions. Initial arena replay revision `fb9b1d5` passed all
three jobs in [CI run 33950710706](https://github.com/titan-engine/titan/actions/runs/33950710706).
CI now includes the RPG replay and browser control acceptance checks.
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
remain deferred. The work is pushed on `main` at `fb9b1d5` with all three CI
jobs green; [results](replay/arena-ci.json) retain the exact tested revision.

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
exercises snapshots and interactive/headless replay; the single PNG loading
exercise does not establish completion of the broader asset requirements. These remain
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
passed that exact revision. Cargo package versions were `0.1.0` at that tag,
consistent with the earlier source milestones. Save/load commits are on `main` and excluded
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
That increment did not publish crates or change package versions. The later
`v0.4.0` release alignment is documented above.

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
broader asset pipeline, parallel executor, or broad reflection redesign work.
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
