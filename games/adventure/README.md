# Two-character adventure

Two cooperative practice rooms for [issues #83](https://github.com/titan-engine/titan/issues/83)
and [#84](https://github.com/titan-engine/titan/issues/84).
The same fixed-tick Rust simulation runs headlessly, in a native GPU window, and
in an actual-WASM browser player. Jumper has a triangle marker; Strong has a
square marker. A yellow outline and the active name identify control.

Jump Jumper onto the raised plate A, switch to Strong and cross the open door
to plate B. Switch back, bring Jumper through to the exit, then bring Strong.
Both complete footprints must be grounded in the exit together. Completion
latches and freezes the room; R or Restart restores it. Room 2 adds a heavy block and a higher ledge requiring both abilities. Select
a practice room explicitly; start/Continue/Play again progression belongs to #85.
The [first-slice design](design.md) specifies the wider intended sequence.

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

WASD/arrows move, Space jumps, Q switches, E plus north/south pushes with Strong,
R restarts the displayed room and P pauses/resumes. N steps a paused
player. Native Escape closes the window. The browser requires canvas focus;
Escape releases it and pauses. Losing focus pauses and clears pending input;
resume is explicit. Space never pauses the browser.

```sh
python3 games/adventure/scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory games/adventure/web
```

Open `/play/`, choose a Practice room, click Play, and focus the canvas.
Changing the selector reconstructs that room and pauses; explicitly resume to play. Add `?backend=webgpu` or
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
Restart takes precedence over switch and movement and selects Jumper.

`state` exposes both foot positions, vertical velocities, grounded flags, support
names, per-tick X/Z/ceiling/landing contacts, static solid bounds, recovery
feedback, `active_character`, effective
`consumed_input`, suppressed actions, session tick/generation and recording
bounds. Recordings retain the raw logical input necessary to reproduce filtering;
maximum length is 4096 ticks. Recordings use fixture `adventure-v3`; older
practice-room recordings are rejected. The optional `room` field selects room 2 for replay (absent means room 1).
The `origin` metadata preserves recovery
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

Select Strong, approach `(5500,0,6500)` and press E with north. A valid push
moves the block one socket immediately and consumes Strong's movement for that
tick. Release E and approach the next stance to push again. Jump Jumper onto
the block, release jump and land, then jump north onto the ledge and plate A.
Exchange the door-holding role at B as in room 1. Either moved socket permits
the ledge jump; the initial socket is too far away. Strong cannot jump onto
the block or reach A, and Jumper cannot push.

Strong must stand on the floor within 100 mm of the point 1 m behind the block.
Pushes require one effective north/south direction and a fresh E press, with
no jump request. Both characters must be clear of the entire swept volume;
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
target/debug/titan --format json --instance adventure input 1 --actions '{"right":{"kind":"button","value":true}}'
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
python3 games/adventure/scripts/build-browser.py
node games/adventure/scripts/test-browser.mjs
node games/adventure/scripts/test-movement.mjs
node games/adventure/scripts/test-puzzle.mjs
node games/adventure/scripts/test-block.mjs
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
alone establishes no browser GPU behavior. See [verification evidence](verification.md)
for the exercised environment and limitations. Build outputs and runtime
diagnostics are ignored, and no external RPG support module is imported.
