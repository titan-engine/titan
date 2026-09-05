# Independent arena exercise

The game builder received the milestone brief, completed minimal starter and
repository-local docs. It copied the starter into `games/arena`, excluding
`target` and generated `pkg`. It used its own components (`Position`, `Player`,
`Enemy`), run resource, input actions, fixed simulation and generated pixel art.
It did not read/import the RPG implementation or history, and did not need to
inspect public engine source beyond the documented starter examples. No engine
API or starter setup blocker was demonstrated. The game-specific host changes
are page/window titles, readable browser status and R restart.

## Diagnosed failed attempt

[The retained response](../games/arena/evidence/failed-attempt.json) points to
[the relocated bounded bundle](../games/arena/evidence/failed-run/bundle.json),
with [API metadata](../games/arena/evidence/failed-run/api.txt) and
[capture](../games/arena/evidence/failed-run/capture.png). The response's absolute
ignored target path was changed to `failed-run/bundle.json` during archival;
the diagnostic payload itself is unchanged. No discovery tokens were copied.

The first attempted survival replay lost at tick495. The bundle shows health0,
outcome Lost, six spawned enemies, player (98,18), and a pursuer at (104,18):
their seven-pixel sprites overlap. Recent accepted inputs end in `right`. This
rules out a missing input adapter or host timing failure: the player moved, but
the route reached the top wall and encountered a pursuer. The original route
also spent excess ticks against arena edges; merely correcting those segments
still lost. Spawn pacing (one per90 ticks) and pursuit (one pixel per3 ticks)
were too aggressive for the intended short introductory slice.

Fix: an inner rectangle route avoids wall stalls; one spawn per240 ticks and
one pursuit pixel per5 ticks keeps the tiny arena navigable. Three health and a
60-tick contact cooldown remain. The final route wins at tick1200 with health2
and five spawned enemies. Idle loss at tick310 proves that survival is not
unconditional. The tests pin both scenarios rather than replacing a failed
expectation with the observed loss.

To reproduce the failed attempt in a temporary copy, set the two simulation
intervals in `src/game.rs` from 240/5 to 90/3, build the native runner, and use
seed41700 with up40 followed by repeating right110, down90, left110, up90. Inject
complete snapshots for ticks1..495, step495, then invoke `verify_survival`.
The command returns `invalid_value` and a diagnostic bundle. The final source
keeps the corrected rules; no deliberate defect is left in the game.

## Reproducible checks

Follow [the arena README](../games/arena/README.md) for native play, browser
build/play, controlled CLI examples and every check. `scripts/test-control.py`
proves discovery, input, exact stepping, validated edits, captures, survival,
loss, restart and bounded native diagnostics. `scripts/test-browser.mjs` runs
actual compiled WASM and verifies control opt-in plus the same winning checksum.
Rust scenarios additionally check exact contact cooldown, pursuit and bounds.

On this machine, native `play --frames 2` presented two GPU frames successfully;
all-target tests, Clippy, actual-WASM scenarios and browser bridge tests passed.
Final visual evidence and user-facing play review are coordinated by the parent
agent. The parent also supplies the GPU readback integration test and root CI.
The maintainer subsequently accepted both playable versions on 2026-09-05.

## Graphical integration evidence

The parent verified actual browser GPU scenarios at `/test/` after building and
serving the arena web directory. The bounded fixture drives the same
`BrowserPlayer`, checks fixed frame counts and asserts restart preserves the
host clock. [Winning canvas](milestone-2/arena-browser-won.png) shows health2 at
20/20 seconds; [losing canvas](milestone-2/arena-browser-lost.png) shows health0.
Win completed at host frame1200; restart plus idle stepping reached host
frame2400 with game elapsed310/Lost; another restart plus240 route ticks reached
host frame2640 with game elapsed240/Running. The main `/play/` page was also
started, and pressing R reset health/time and paused with Resume available.

Native `play --frames 6000` presented6000 frames and exited normally. The opt-in
`cargo test --test gpu -- --ignored` compared the arena's initial, active and
loss scene pixels exactly with software output, using both unorm and sRGB GPU
targets. These are integration results, not substitutes for exact software
checksums or gameplay assertions. [Verification summary](milestone-2/verification.json).

A [fresh independent verifier](arena-verification.md) reproduced native and WASM
checks using local guidance. No engine expansion was needed. The maintainer played both native and browser
versions and accepted the result on 2026-09-05.

## Reusable macOS host gap

The native executable initially rendered correctly but was absent from Computer
Use's app list and could not be selected by executable path. At the maintainer's
suggestion, the starter gained a reusable `scripts/build-macos-app.py` step,
carried into the arena copy. It packages Cargo's reported executable and a
standard Info.plist as an unsigned local `.app`, with configurable name and
bundle ID. This changes host packaging, not simulation or rendering.

Computer Use then selected `Titan Arena.app`, exposed the native window,
produced [a native screenshot](milestone-2/arena-native-app.png), and exercised
R restart and the window close button. The process exited after closing.
`scripts/test-macos-bundles.py` independently copied both projects outside the
checkout, built distinct bundles, relocated/renamed them, and ran their embedded
executables with `--help`. A macOS CI job runs that check. This is a development
bundle, not a signed/notarized distribution package.
