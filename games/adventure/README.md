# Two-character adventure

A standalone movement prototype for [issue #82](https://github.com/titan-engine/titan/issues/82).
The same fixed-tick Rust simulation runs headlessly, in a native GPU window, and
in an actual-WASM browser player. Jumper is narrow with a triangle marker;
Strong is broad with a square marker. A yellow ground outline and the active
name identify control independently of body color.

This increment provides a 12 × 8 metre practice room, jumping, swept static
collision, switching, inspection and recording. A 1 m teaching ledge, 2 m high
ledge, fixed 0.75 m practice step and ceiling exercise movement. The step is
static; block pushing, plates, doors and progression belong to later issues. The [first-slice design](design.md) describes
the intended puzzle rules, not features already present here.

## Play locally

From the repository root:

```sh
cargo run --manifest-path games/adventure/Cargo.toml --features player --bin play
# Bounded GPU run:
cargo run --manifest-path games/adventure/Cargo.toml --features player --bin play -- --frames 120 --run-for-ms 10000
# Optional unsigned local macOS bundle:
python3 games/adventure/scripts/build-macos-app.py --name "Titan Adventure" --bundle-id dev.titan.adventure
```

WASD/arrows move, Space jumps, Q switches, R restarts and P pauses/resumes. N steps a paused
player. Native Escape closes the window. The browser requires canvas focus;
Escape releases it and pauses. Losing focus pauses and clears pending input;
resume is explicit. Space never pauses the browser.

```sh
python3 games/adventure/scripts/build-browser.py
python3 -m http.server 8000 --bind 127.0.0.1 --directory games/adventure/web
```

Open `/play/`, click Play, and focus the canvas. Add `?backend=webgpu` or
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
maximum length is 4096 ticks. Recordings use fixture `adventure-v2`; older
flat-room recordings are rejected. The `origin` metadata preserves recovery
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

To try the teaching ledge, walk Jumper north from its start for 50 ticks
(to Z=3.5 m), then jump north for 22 ticks and release movement to land.
Strong cannot reach that 1 m top. The static step supports practice of the
remaining 1.25 m rise onto the high ledge. This practice arrangement combines
geometry for movement verification; it is not either completed puzzle room.

Ordinary missed jumps land safely. A defensive foot Y below -2 m reconstructs
both characters on that tick, selects Jumper, clears velocity and pending input,
and displays “Fell - room reset” for 120 simulation ticks. R reconstructs without
the message. The room tick resets while host frame and reset generation retain
provenance. Controlled below-floor fixtures run only in the optional
`movement-acceptance` build; players expose no teleport control.

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
node --test games/adventure/web/play/*.test.mjs
python3 games/adventure/scripts/test-player.py
```

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
