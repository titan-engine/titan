---
name: titan-workflow
description: Build, run, inspect, replay, and diagnose Titan games using the standalone starter and the RPG regression example. Use for game iteration and engine changes that need runtime evidence.
---

# Titan game iteration

Run commands from the repository root. Read `docs/verification.md` for
quality gates and follow `docs/workflow.md` plus the GitHub board for current
priorities. Read `docs/cli.md` for flags and structured output. Use
`docs/ecs-authoring.md` when editing systems, `docs/browser.md` for WASM, and
`docs/rendering.md` for interactive or GPU work. These paths are relative to the
repository root, three directories above this skill folder. Follow
`docs/acceptance-evidence.md` for evidence retention: full sanitized run bundles
go to ignored output or external temporary directories; promote selected
baselines/illustrations only through explicit review. This preserves all checks
and capture inspection obligations.

## New games

Start with `starters/minimal/README.md`: copy the standalone package, configure
its Titan path dependencies, and put game definitions in `src/game.rs`. The
starter has its own native controlled runner, GPU player, browser adapters and
portable build script. It does not import RPG support code. Shared host APIs
are documented in `docs/host-tooling.md#host-boundaries` and `docs/rendering.md`: surface presentation, browser inspection policy, PNG
encoding and build packaging are reused; game inputs and presentation stay local.
For native macOS visual verification, use the package's
`scripts/build-macos-app.py --name "My Game" --bundle-id dev.example.my-game`.
The README documents this unsigned local app bundle; select its printed absolute
path in Computer Use to launch and inspect the native player.

`python3 scripts/test-starter.py` verifies a copy outside the repository.
`python3 starters/minimal/scripts/build-browser.py` and
`node starters/minimal/scripts/test-browser.mjs` exercise its actual WASM.
Native CLI inspection works identically after selecting the copied project's
path and instance. Browser inspection uses its local page/message bridge, not
native discovery. Keep game state and recent inputs useful in diagnostic bundles.

Read the copied starter's README for its input, restart and field semantics;
the RPG's tile pulses and reference route below are not generic game rules.

## RPG regression

Build the native control tools with:

```sh
cargo build -p titan-cli -p titan --bin titan --example procedural_rpg
```

The RPG hosts load `assets/player.png` and `assets/tree.png` at startup. Use `--assets-dir DIR` for
another directory or `--generated-assets` for the procedural comparison. In-game
restart retains loaded art. See `docs/assets.md` for browser copies, bundle lookup,
startup diagnostics and image-aware replay verification. Native bundle inspection
uses `python3 scripts/build-rpg-app.py` and its printed absolute path.

Launch a bounded paused game in a separate process:

```sh
target/debug/examples/procedural_rpg --serve --instance iteration --run-for-ms 120000
```

Use `target/debug/titan --format json --instance iteration` followed by
`capabilities`, `status`, `entities`, or `commands`. Capabilities and command
metadata describe what this runtime supports. Multiple matching instances require
an explicit selection. Discovery files contain bearer tokens: use CLI instance
output for reports and do not copy the raw registry into artifacts.

Inputs are complete snapshots for a future fixed frame, not incremental changes:

```sh
target/debug/titan --format json --instance iteration input 1 --actions '{"right":{"kind":"button","value":true}}'
target/debug/titan --format json --instance iteration step 1
target/debug/titan --format json --instance iteration capture
```

Field edits additionally require native `--allow-mutation` at launch (browser
control opt-in covers edits). Read the entity's `component_fields` metadata and
use its exact qualified component key with `set-field INDEX GENERATION COMPONENT
FIELD --value JSON`. Registered getters expose values in `components`; metadata
states types, bounds, and writability. A CLI flag cannot enable a running host's
mutation policy. Invalid types/bounds are rejected before assignment.

Read `observed_frame`, `state_revision`, and the structured outcome. Failed
operations may have partially changed state; unchanged revision means no
successful operation was recorded, not transaction rollback. Do not blindly
retry a timed-out mutation: inspect first because it may already have applied.
On failure, read `error.details.diagnostic_bundle` when present. Its manifest,
`api.txt`, and optional `capture.png` provide local evidence; inspect logged
capture/write failures rather than assuming every artifact exists.

`python3 scripts/test-control-loop.py` exercises discovery, exact replay,
inspection, commands, captures, diagnostic failures, and shutdown in separate
processes. The reference route is right 2, down 3, right 6: eleven fixed ticks,
three collected shards, active shrine, software RGBA checksum
`f7a298f62ad75c1c`. Preserve it for behavior-neutral engine changes. For intentional
visual changes, compare before/after images and semantic results before updating
an expected checksum; a new checksum alone is not evidence of improvement.

After an engine change, use the quality gates listed in `docs/verification.md`.
Ordinary `cargo test` remains useful; the Titan CLI adds structured results and
bounded execution. Stop owned runtime processes after verification and let their
normal shutdown remove discovery registrations.


## Collection-room 3D loop

Read `games/collection-room/README.md` for the native/browser players, fixed
44-tick route (right 8, up 20, right 16), replay and capture commands. Native
inspection requires `--inspect`; tool-driven changes also require
`--allow-control`. Browser inspection targets the actual played instance through
`window.collectionRoom.dispatch`, an awaited schema-2 Promise. Its control
checkbox defaults off; captures remain available read-only.

Pause, await edits/steps, then capture. Keep the full response JSON alongside the
PNG: its identity records the accepted host tick, revision and reset generation.
A capture freezes fresh scene and ECS overlay data without ticking; paused
teleport/capture must change the image at the same tick. Restart invalidates
pending old-session captures. The fixed 960 × 540 capture is independent of
surface resizing. Headless hosts intentionally have no 3D capture capability.

Run `python3 games/collection-room/scripts/test-player.py` for native GPU/control
acceptance, build with its `scripts/build-browser.py`, then serve its `web/` and
open `/play/test.html?backend=webgpu` and `?backend=webgl2` in an actual browser.
Inspect the captured images, including the deliberate occluded and visible player
poses. Use the game README's declared GPU tolerances; semantic state remains exact.
Keep bounded sanitized local evidence and stop owned servers/players. A passing
Node/WASM test is semantic evidence, not a browser GPU verification.
