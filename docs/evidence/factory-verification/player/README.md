# Player exercise record

Source SHA: `e4800939606889669e8a9b04650cda4bce6df37d`. Date: 2026-09-06.
Native app built with `python3 games/factory/scripts/build-macos-app.py --name
"Titan Factory Verification" --bundle-id dev.titan.factory.verification`.
Browser built with `python3 games/factory/scripts/build-browser.py`, served from
`games/factory/web` on localhost port 8093. No source behavior was changed.

## Manual native and browser play

The evaluator clicked palette controls and tiles to build extractor (1,3),
conveyors (2..4,3), processor (5,3), conveyors (6..9,3), initially all east.
The last belt was rotated twice to west. Inspecting belt (8,3) exposed the
receiver (9,3) wrong-facing remedy. Paused native tick 2160 and browser tick 1800
both showed a backed-up line and no deliveries. Rotating (9,3) twice more to east
then running/stepping completed native at 2769 and browser at 2409. Native was
built after an empty-running interval, so these are explicitly different histories,
not a native/browser clock parity comparison. The browser history starts with
construction at zero and is reproducible as build; rotate twice; advance 1800;
rotate twice; advance to completion at 2409.

Images: [native blocked](native-manual-blocked.png),
[native complete](native-manual-complete.png),
[browser blocked](browser-manual-blocked.png),
[browser complete](browser-manual-complete.png). The displayed arrows, inventory
markers, pinned explanation and delivered count were visually inspected.
Native Step and a construction click after completion retained tick 2769 and
showed `Complete: restart to build a new factory`. Browser Step/Resume disabled.

A native restart cleared the route; a newly placed isolated extractor produced
one ore and blocked. Pause → Remove palette → right-click inspect displayed
[exact one-ore discard preview](native-removal-preview.png). Clicking removed it,
leaving empty deposit and total discarded=1. Tab/Enter selected the Conveyor
palette. A corner drag did not visibly resize the window; macOS window zoom made
a small height/layout change with retained controls. No broad native size matrix
is claimed. Escape followed by querying the app reacquired a running app, so the
owned process was explicitly stopped; later fixture close used the window control.

## Known-state native capture

Run the bundle executable (or `games/factory/target/debug/play`) with
`--test-interface --frames 6000`, retaining stdout. It first exercises its existing
host fixture, including completion at 1269, then presents paused tick 65 with the
processor facing south. This reused fixture supplements, rather than replaces,
the independent manual route above. [Initial JSON](native-known-state.json)
contains the fixture result and authoritative state; [image](native-known-state.png)
shows belt (4,3) with ore, wrong-facing processor and the actionable explanation.

Choose Rotate, click processor three times, click Step once. [Repaired image](native-known-repaired.png)
shows tick 66, empty upstream belt and occupied processor. Close the window;
[final stdout state](native-known-repaired.json) confirms processor facing east,
in-process ore and remaining=120. It reported 4,679 presented GPU frames.
Captures are app-window screenshots, not GPU readback identities/checksums. The
paused game state is pinned by fixture stdout and the final exit state; no native
live-inspector claim is made.

## Independent browser exercise and parity

This is a historical browser run at the source SHA above. The
[original DOM harness](https://github.com/titan-engine/titan/blob/17723e62334a19763f8cf81b2f31cc840b4d6289/docs/evidence/factory-verification/player/browser-exercise.html) remains available at
its evidence-containing revision. For a historical rerun, retrieve that revision's
harness into a disposable checkout of the measured source, build the browser,
copy the harness to `games/factory/web/independent-verification.html`, and serve
`games/factory/web` on localhost port 8093. Keep resulting captures in ignored
`target/evidence/` or outside the maintained checkout.

Open `/independent-verification.html` in an actual GPU browser. It runs a bounded
DOM exercise against the actual player in an iframe and publishes `PASS independent
factory` plus JSON. It uses the documented test-only read hook for authoritative
state; construction/rotation/removal/steps use DOM controls and pointer events.
It independently counts every item each tick rather than trusting `conserved`.
It checks visible diagnosis text against expected repair meaning. No fixture
seeding or private-state mutation is used.

The first recipe-repair attempt failed because four downstream belts still held
ore. The final harness explicitly removes/rebuilds those belts. At tick 1200,
there are four deliveries and five ore discarded. Removing the processor then
adds two ore discards (queued and in-process), replacement recovers, and completion
occurs at 1926 with 22 extracted, seven discarded and five resident items. After
restart, a south-facing processor stalls at tick 600. Three clockwise rotations
recover, completing at 1806 with zero discards. [Saved JSON](browser-exercise.json)
and [fresh repeat](browser-exercise-repeat.json) preserve all seven states.
They match exactly after excluding only `frame` (pre-restart startup differed).

[Original native verification](https://github.com/titan-engine/titan/blob/17723e62334a19763f8cf81b2f31cc840b4d6289/docs/evidence/factory-verification/player/verify-traces.py) replays the same repair history through
3,767 operation boundaries and compares all seven browser states after excluding
only host/UI fields listed in [its result](native-browser-traces.json). It asserts
conservation at every boundary and no rejected operation. [Exact sequence](repair-sequence.json)
is retained. The independent simulation result confirms UI-driven semantics.

The maintained [native regression](../../../../games/factory/scripts/verify-traces.py)
now consumes only seven read-only semantic states in
[`repair-browser-checkpoints.json`](../../../../games/factory/tests/fixtures/repair-browser-checkpoints.json).
Run the [current documented command](../../../../games/factory/README.md#source-and-checks)
to compare HEAD native behavior with that historical browser baseline. Its summary
and generated sequence default to root `target/evidence/factory-repair/`; an
explicit `--output-dir` can select another ignored or external directory. This
headless rerun does not establish fresh browser/GPU or manual-player evidence.


The 900px layout control narrows the actual player iframe. [Screenshot](browser-900.png)
shows readable wrapped palette text, objective and pinned inventory. Restart,
Zoom+ and Pan right remained clickable after scrolling at that size. A read-only
snapshot confirmed tick 0, one fixed delivery and camera (-16,0,1.2):
[layout state](browser-layout-state.json). The ordinary player was also inspected
at 1280×720. A viewport override initially did not affect the previously selected
tab, so its unchanged 1280 width was checked before using the actual narrow fixture;
no false 900px capture was recorded. The browser override was reset afterward.

## Tooling observations and limits

A direct attempt to read the test hook through computer-use's isolated read-only
page scope failed (`titanPlayerTest` unavailable there). The retained same-origin
HTML publishes its state to ordinary DOM text instead. One manual browser step
loop tried to click again after completion and hit the disabled control; inspecting
the page confirmed completion at 2409, and no mutation retry was needed. These are
evaluator/tooling outcomes, not gameplay failures. Builds succeeded after normal
cache population, including install of the matching wasm-bindgen helper.

This exercise did not measure UI latency, FPS, accessibility with a screen reader,
every browser backend, arbitrary OS window sizes or human learning time. Native
controls were not exposed individually through the accessibility tree. Screenshot
observations and machine-state assertions are separate from the larger-fixture
measurements and historical skeleton baseline.
