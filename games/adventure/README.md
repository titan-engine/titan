# Two-character adventure

A short two-room cooperative game for [issue #85](https://github.com/titan-engine/titan/issues/85).
The same fixed-tick Rust simulation runs headlessly, in a native GPU window, and
in an actual-WASM browser player. Jumper has a triangle marker; Strong has a
square marker. A yellow outline and the active name identify control.

Jump Jumper onto the raised plate A, switch to Strong and cross the open door
to plate B. Switch back, bring Jumper through to the exit, then bring Strong.
Both complete footprints must be grounded in the exit together. Start explains
the controls and selects Jumper in room 1. Room completion freezes the puzzle
until Continue or Enter starts room 2 with Jumper and fresh puzzle state. Room 2
adds a heavy block and a higher ledge requiring both abilities. Slice completion
offers Restart room and Play again; Play again starts room 1. R always restores
the displayed room. The [first-slice design](design.md) specifies the rules.
The [next-milestone proposal](../../docs/adventure-next-milestone.md) awaits
maintainer selection; further gameplay and engine work are not approved.

## Play locally

From the repository root:

```sh
cargo run --manifest-path games/adventure/Cargo.toml --features player --bin play
# Start the combined-abilities practice room:
cargo run --manifest-path games/adventure/Cargo.toml --features player --bin play -- --room 2
# Bounded GPU run:
cargo run --manifest-path games/adventure/Cargo.toml --features player --bin play -- --frames 120 --run-for-ms 10000
# Optional unsigned local macOS bundle:
python3 games/adventure/scripts/build-macos-app.py --name "Titan Adventure" --bundle-id dev.titan.adventure
```

WASD/arrows move, Space jumps, Q switches, hold E plus north/south to push with Strong (release E after each push),
R restarts the displayed room and P pauses/resumes. N steps a paused
player. Native Escape closes the window. The browser requires canvas focus;
Escape releases it and pauses. Losing focus pauses and clears pending input;
resume is explicit. Space never pauses the browser.

```sh
python3 games/adventure/scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory games/adventure/web
```

Open `/play/`, initialize the player, then choose Start or press Enter with the
canvas focused. Add `?backend=webgpu` or
`?backend=webgl2` to select a backend explicitly. Initialization errors are
reported; there is no software 3D fallback. The camera remains at `(6,14,17)`,
looking at `(6,0,4)`, with a 50-degree vertical field of view. Presentation uses
16:9 with black bars where necessary; capture resolution is always 960 × 540.
The room does not track the active character or pan on switching.

## Simulation and input contract

Positions are integer millimetres, fixed time is 60 ticks/s, axial displacement
is 60 mm/tick and diagonal displacement is 42 mm per axis. Opposing directions
cancel. W/up moves north (-Z); D/right moves east (+X). Character foot centers
stay within X `[200,11800]` and Z `[200,7800]`. Characters can share space and do
not push each other. The inactive character receives no horizontal movement; gravity and landing
continue while airborne. Grounded inactive characters stay put.

The initial design's release-gating policy is retained: switching consumes the
switch tick without movement. Held movement remains suppressed until its
logical action is released; a fresh unrelated direction works immediately on a
later tick. Holding Q never repeats a switch. Physical aliases are combined
before this policy. Keyboard and injected input use the same filtering.
Restart takes precedence over confirmation, switching and movement and selects
Jumper. Start, Continue and Play again consume their action without movement,
jumping or pushing; held actions require release/repress in the new room.
Room transitions discard pending input and invalidate old pending captures.

`state.phase` distinguishes `start`, `playing`, `room_complete` and
`slice_complete`. `state` also exposes both foot positions, vertical velocities, grounded flags, support
names, per-tick X/Z/ceiling/landing contacts, static solid bounds, recovery
feedback, `active_character`, effective
`consumed_input`, suppressed actions, session tick/generation and recording
bounds. Recordings retain the raw logical input necessary to reproduce filtering;
maximum length is 4096 ticks. Recordings use fixture `adventure-v3`; older
practice-room recordings are rejected. The optional `room` field selects the recording origin room (absent means room 1).
Recordings begun on Start retain the complete sequence through Continue and
Play again; room-only recordings remain supported.
The `origin.phase` field is `start` for a sequence recording and defaults to
`playing` when absent in an existing room recording. `origin` also preserves recovery
feedback and held-input gates for recordings started after a defensive reset. A truncated or invalid recording is rejected
before changing the app. Restart resets the session, pending input and recording
while retaining the monotonic host frame. Player replay supports pause, step,
resume, restart and returning to live play on restart.

## Jumping and support

Bodies have a 400 × 400 mm footprint and 900 mm height. Jumper's narrow visual
body fits within that footprint; Strong fills it. Space requires a fresh press
while supported. Jumper launches at 180 mm/tick and Strong at 100; gravity
subtracts 10 before each airborne vertical step, giving exact apices of 1530
and 450 mm. Holding Space never repeats, and airborne presses are not buffered.
Horizontal collision sweeps X then Z before the vertical sweep. There is no
step-up, character stacking, coyote time, or general physics solver.

Any positive footprint overlap supports the body; touching only an edge does
not. Walking off starts gravity that tick. Descending onto a ledge or the floor
lands on the highest crossed support. Ceiling contact stops upward velocity.
Solid extents drive both collision and meshes; exterior walls use a 0.3 m visual
cutaway with 4 m collision, and the foreground wall is omitted for visibility.

To reach plate A comfortably, move right for 8 ticks, north for 50, then jump
north for 25 and release to land. The ledge is 1 m high, above Strong's 0.45 m
jump. North and south partitions and the closed door collide to 4 m; the
partition meshes use a cutaway so the far side remains visible.

## Cooperative room rules

Plate A is the 600 × 600 mm square centered at `(2000,1000,2000)`; B is centered
at `(10000,0,5000)`. A character presses a plate only when grounded at its support
height with its foot center inside (including boundaries). The inactive partner
continues to hold it. Either plate requests the door open. Collision uses the
previous tick's door state, so a fresh press permits passage on the next tick.

With no plate pressed, positive body overlap with the doorway holds it fully
open, including airborne bodies. Exact face contact alone permits closing.
The door never crushes or shoves a character. Inspection exposes
`puzzle.plates` with named occupants and `puzzle.door` with `open` and `state`:
`closed`, `open_plate`, or `open_obstructed`. `puzzle_geometry` exposes the exact
plate/exit rectangles and full-height door bounds. Plate and door symbols share a link
mark; the HUD also reports pressed/open/obstructed states in text.

`puzzle.exit.jumper` and `.strong` require grounded, complete 400 × 400 mm
footprints inside X `[10000,12000]`, Z `[1000,3000]`. `puzzle.complete` latches
only when both qualify on the same tick. Movement, switching and puzzle time
then freeze. Restart remains available and reconstructs both characters,
plates, door, exit indicators, completion and input/recording state.

Ordinary missed jumps land safely. A defensive foot Y below -2 m reconstructs
both characters on that tick, selects Jumper, clears velocity and pending input,
and displays “Fell - room reset” for 120 simulation ticks. R reconstructs without
the message. The room tick resets while host frame and reset generation retain
provenance. Controlled below-floor fixtures run only in the optional
`movement-acceptance` build; players expose no teleport control.

## Combined-abilities room

Room 2 has a 2 m ledge at X `[4000,7000]`, Z `[1000,3000]`. Its plate A is
centered at `(5500,2000,2000)`. A 900 × 900 × 750 mm block starts at `(5500,0,5500)`
and moves between north/south sockets at Z 5500, 4500 and 3500. Socket markers
are flat guides; only the block provides support. The door, far plate and exit
use the room 1 rules.

Select Strong, align centrally with the south face and hold E with north.
Either key order works: walk against the block then tap E, or hold the
combination while approaching. A valid push
moves the block one socket immediately and consumes Strong's movement for that
tick. Release E after each successful push, then hold E and approach again. Jump Jumper onto
the block, release jump and land, then jump north onto the ledge and plate A.
Exchange the door-holding role at B as in room 1. Either moved socket permits
the ledge jump; the initial socket is too far away. Strong cannot jump onto
the block or reach A, and Jumper cannot push.

Strong must stand on the floor within 250 mm laterally of the block centre
and 650–1100 mm behind it in the push direction (650 mm is natural contact).
Pushes require held E and one effective north/south direction, with no jump
request. Failed requests retry while held, so approach needs no timing; success
locks E until release/repress, preventing automatic repeated pushes. Both characters must be clear of the entire swept volume;
characters supported by the block prevent moving it. Positive overlap blocks
movement, while exact face contact is clear. Rejected pushes leave the block
unchanged; ordinary movement/jumping still runs. Inspection's `block` reports
socket, move count and `last_rejection`, using this priority:
`wrong_character`, `not_grounded`, `invalid_direction`, `invalid_stance`,
`rail_end`, `block_occupied`, `path_obstructed`. The HUD shows rejection feedback.
`room`, `block_geometry` and room-specific `solids`/`puzzle_geometry` expose the
collision state. Dynamic support has the stable name `heavy-block`.

At the intermediate socket, Strong can walk around the east side and push south
to recover the initial arrangement. The final socket's reverse stance is inside
the ledge, so restart restores it. Missed jumps preserve the arrangement; R and
defensive fall recovery reconstruct the current room, including block, puzzle,
input gates and recording origin.

The [two-push route](tests/block-solution.json) and
[one-push route](tests/block-intermediate-solution.json) are ordinary-input
solutions. `test-block.mjs` compares native and actual-WASM state at every tick,
including adversarial fixtures. Both GPU player harnesses exercise both routes
and capture their support, plate and completion checkpoints.

## Inspect and control

```sh
cargo build -p titan-cli
cargo run --manifest-path games/adventure/Cargo.toml -- --serve --instance adventure --run-for-ms 120000
# In another terminal, using the directory the host was launched from:
target/debug/titan --format json --instance adventure query state
target/debug/titan --format json --instance adventure invoke confirm
target/debug/titan --format json --instance adventure input 2 --actions '{"right":{"kind":"button","value":true}}'
target/debug/titan --format json --instance adventure step 1
target/debug/titan --format json --instance adventure invoke switch
target/debug/titan --format json --instance adventure query recording
target/debug/titan --format json --instance adventure invoke restart
target/debug/titan --format json --instance adventure invoke select_room --arguments '{"room":2}'
```

Inputs are complete button snapshots for a future host frame. An omitted action
is released. `switch` executes one recorded fixed tick, including held-input
filtering. `restart` reconstructs the session without advancing the host clock.
`entities` and `entity` expose named `jumper` and `strong` positions as read-only
fields. There is no teleport command or writable gameplay field.

For inspection of the actual native window, add `--inspect --allow-control`
to the `play` binary. Omit `--allow-control` for read-only inspection and GPU
captures. The browser player exposes `window.adventure.dispatch(json)` on the
played instance; await its schema-2 response Promise. The visible control
checkbox defaults off and gates injected input, stepping and commands. The
`BrowserRuntime` adapter used by the Node acceptance script is a separate paused
CPU instance; this package provides no separate inspector webpage.
CPU-only hosts intentionally report capture unsupported. Native/player captures
freeze fresh scene and HUD data with frame/revision/reset identity and require
no simulation tick. See [inspection](../../docs/inspection.md) for the protocol.

## Reproduce verification

```sh
cargo fmt --manifest-path games/adventure/Cargo.toml --all --check
cargo test --manifest-path games/adventure/Cargo.toml --all-targets --all-features
cargo clippy --manifest-path games/adventure/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/adventure/scripts/test-control.py
python3 games/adventure/scripts/test-playtest.py
python3 games/adventure/scripts/build-browser.py
node games/adventure/scripts/test-browser.mjs
node games/adventure/scripts/test-movement.mjs
node games/adventure/scripts/test-puzzle.mjs
node games/adventure/scripts/test-block.mjs
node games/adventure/scripts/test-sequence.mjs
node --test games/adventure/web/play/*.test.mjs
python3 games/adventure/scripts/test-player.py
```

The [solution segments](tests/puzzle-solution.json) and
[versioned recording](tests/puzzle-recording.json) complete the room using only
normal input. The puzzle acceptance runner compares native and actual WASM
traces for the solution and controlled adversarial fixtures.

The [control fixture](tests/control-route.json) asserts every tick's selected
character and both positions. The WASM test executes a fresh native CLI trace
and compares the complete per-tick states with actual WASM in Node. Rust and
browser key tests additionally cover aliases, focus loss, restart, opposing
input and replay boundaries. The native GPU script retains identity-bearing
captures and verifies interactive replay on the inspected player.

With the browser server running, visit `/play/test.html?backend=webgpu` and
`?backend=webgl2`. Each must independently report a pass; Node WASM execution
alone establishes no browser GPU behavior. Historical platform observations and
their limitations are recorded below. Build outputs and runtime diagnostics are ignored, and no external RPG support module is imported.

## Bounded source variations

Room geometry is ordinary Rust source: public `Rect`, `PLATES` and `plates(room)`
in [game/puzzle.rs](src/game/puzzle.rs) define plate bounds; room-specific
solids are in [game.rs](src/game.rs). Inspect `state.puzzle_geometry` to confirm
the compiled bounds. Work in a disposable copy for an experiment, rebuild the
game, adapt ordinary input routes, and verify both completion and replay.
The historical plate variation below retains an immutable patch, route and
reproduction command. The [iteration procedure](../../docs/agent-iteration.md) defines measurement and
diagnostic reporting; this example adds no runtime editing or new mechanics.

## Historical exercise provenance

The independent #86 evaluation measured gameplay source
`02272893a0d91af2b1ac6b5159644b70ab46108c` on 2026-09-06, macOS / Apple M5 Pro.
Its [original report](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/games/adventure/evidence/playtest-86/README.md)
and adjacent native/WebGPU/WebGL2 identity-bearing reports and inspected PNGs
remain available at evidence revision `e4ff0dff2d02dfffa6bc085286798886a92e30e7`.
Native Metal presented 4,311 frames; actual browser WebGPU and WebGL2 each
passed 209 checks independently. Three perturbed semantic scenarios replayed
34 checkpoints. These bounded agent checks found no gameplay defect; they do
not establish human discoverability, current GUI behavior, portable pixel
identity or exhaustive freedom from softlocks. The fixed view and held-switch
release gesture were observed limitations. The current commands above remain
authoritative for reruns; the native playtest runner uses solution routes as
navigation templates, adding excursions, waits, recovery and transition probes.
It writes its state/operation report to ignored
`games/adventure/target/playtest-86/semantic.json` by default. Native GPU captures
use ignored root `target/adventure-gpu-evidence/`; save browser downloads and their
capture identities to ignored output such as root `target/evidence/adventure/`.
Check custom output paths with `git check-ignore` before running.

The unfamiliar-author variation moved room-1 plate B south 600 mm and completed
a 579-tick ordinary-input route with final-state replay, invalid-recording
rejection and room-2 preservation. It measured the same source with the
[original patch and route](https://github.com/titan-engine/titan/tree/3e584a023d346805c23c2bf11162c87dee5042ed/games/adventure/evidence/playtest-86).
The [method and limitations](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/games/adventure/evidence/playtest-86/variation-notes.md)
support #97's assessment: full-task monotonic timing was missed, the observed
UTC interval was 244 seconds, and successful command phases totalled 15.044
seconds with uncertain cache warmth. This is one CPU-only authoring sample,
not a full-task latency or browser/GPU result. Reproduce outside the checkout:

```sh
scratch=$(mktemp -d)
evidence_root=$(mktemp -d)
git archive 3e584a023d346805c23c2bf11162c87dee5042ed games/adventure/evidence/playtest-86 | tar -x -C "$evidence_root"
evidence="$evidence_root/games/adventure/evidence/playtest-86"
git archive 02272893a0d91af2b1ac6b5159644b70ab46108c | tar -x -C "$scratch"
patch -d "$scratch" -p1 < "$evidence/variation.patch"
CARGO_BUILD_JOBS=4 python3 "$evidence/variation-reproduce.py" "$scratch" "$scratch/results"
```

The later #124 contact-pushing correction has separate
[human recording, final state and capture provenance](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/games/adventure/evidence/push-124/human-result.json)
and [platform verification](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/games/adventure/verification.md#contact-pushing-correction-124).
On 2026-09-06 the maintainer completed room 2 at tick 1042 with ordinary keyboard
input in the native Metal release player on Apple M5 Pro; inspection was read-only.
The implementation revision is `bf9835541d5acfe789f20d8a37cfc28aef6ca743`;
the played build had the same gameplay code before the HUD plus-sign became AND
and focused tests were added. The 1165-frame recording includes frozen completion
frames and matched native/actual-WASM final-state replay. The separately recorded
platform pass reports native Metal and 229 checks each in actual WebGPU/WebGL2.
This is one maintainer playtest, not a broader usability study. Use the current
block/sequence and player commands above for new checks; historical recordings
and their original reproduction context remain at the linked evidence revision.
