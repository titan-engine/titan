# Interactive arena replay

The arena plays the same bounded input recording in its native window, browser
canvas and headless verifier. Each new recording includes a starting snapshot,
so recording works immediately after restoring a mid-run save.

## Local controls

In the browser Play page, start the player, pause, then choose **Load recording**.
Use Play/Pause, **Step one tick**, **Seek to tick**, **Playback speed**,
**Restart playback**, and **Exit playback**.
The status shows the current recorded tick, total ticks and verification result.
Local playback controls do not require enabling remote inspection controls.
Files remain local; imports are limited to 2 MiB and reject a session change
while the asynchronous file read is in progress.

From the repository root, start a native recording paused:

```sh
cargo run --manifest-path games/arena/Cargo.toml --bin play -- --recording /tmp/arena-recording.json
```

P plays/pauses, N advances one recorded tick while paused, R restarts playback,
and L exits to a fresh live game. While paused, Left/Right seek backward/forward
60 ticks, Home/End seek to the bounds, and -/+ select slower/faster playback.
The window title shows progress, speed and completion.
Native imports require a regular JSON file no larger than 2 MiB.

Playback ignores physical movement, dash and pointer input. The in-game restart
button is disabled. Exiting playback starts a fresh live game with a new recording;
it does not resume from the last replay frame. Restarting playback restores its
initial snapshot. These transitions clear pending input and wall-clock catch-up
without rewinding the host frame or replacing inspection identity.

## Seek and speed semantics

Seek positions are consumed recording ticks, inclusive from zero through the
recording length, independent of the starting gameplay tick and monotonic host
frame. Seek and speed changes require pause. Forward seeks replay from the
current position; backward seeks restore the validated origin and replay from
zero. A new seek replaces a pending target. Each host update advances at most
120 seek ticks, returns control, and exposes the current position and target.
Seeking remains paused, including on arrival; resume and manual steps cannot
advance a pending seek. At EOF the complete snapshot and pixels are verified
again. Seeking away clears that previous completion result.

Speeds are ¼×, ½×, 1×, 2× and 4×. They scale elapsed playback time, leaving
fixed tick duration, recorded input and manual one-tick steps unchanged. Hosts
cap incoming elapsed time at 250 ms and execution at 120 ticks per update.
Seek/speed transitions clear accumulated time and pending physical/control input.
Browser animation updates pump seeks while paused; zero-elapsed render calls
used by resize and inspection do not. Native seeks progress at event-loop safe
points even when presentation is suspended. The cap bounds tick count rather
than promising a wall-clock latency independent of game-system costs.

## Inspection and verification

Inspection, entity listing, save queries and captures stay available throughout
playback. `arena_state.replay` reports `active`, `position`, `total`, `complete`,
`verified`, `error`, `seeking`, `target` and `speed`. Verification is reported when playback reaches its end;
the player pauses automatically without consuming another simulation tick.

The interactive session exposes commands `load_replay` with
`{"recording": <recording>}`, `restart_replay`, `stop_replay`, `seek_replay` with `{ "position": 120 }`,
and `replay_speed` with `{ "speed": 0.5 }`. Remote commands
require inspection control permission and pause. Existing pause/resume and Step
operate on recorded frames; a Step request exceeding the remaining recording is
rejected before advancing. Ordinary remote `restart` exits to a fresh live game.
Input injection, field edits, save loading and UI pointer commands are rejected
during playback. The isolated Inspector/standalone runner offers headless
verification rather than these interactive playback commands.

Imports are fully validated and replayed in a fresh game before replacing the
current scene. Invalid structure, unsupported identity, inconsistent snapshots,
invalid action edges and final-state/pixel mismatches leave the current session
unchanged. Visible playback independently verifies again at completion.

For headless verification:

```sh
cargo run --manifest-path games/arena/Cargo.toml --bin replay -- /tmp/arena-recording.json
```

Both import paths accept a raw exported recording or the CLI recording-query
response envelope. CLI `--arguments-file` can send a wrapped recording to
`load_replay`; its separate 1 MiB argument-file bound still applies.

## Recording boundary

Format v2 embeds `initial_snapshot` and `final_snapshot` using the arena's
[game-owned save format](arena-save-load.md). It retains the game seed, action
schema, fixed-step duration, source host frame, consumed input frames, summarized
final state and software RGBA checksum. Full final-state comparison includes RNG,
enemy slots, dash/contact cooldowns and other private gameplay state; host time
is deliberately excluded. Input frames preserve active, pressed and released
actions, including held input at a snapshot boundary.

Restart and successful save loading each start a new recording segment. During
playback, querying/exporting the recording returns the original loaded artifact.
Recordings are capped at 3,600 ticks (60 seconds) and 2 MiB on import. Truncated
or externally edited recordings cannot establish exact replay.

Historical format v1 recordings remain supported with their original fresh-game
origin. The checked-in compatibility fixture predates v2; this narrow reader
support does not promise future game/save compatibility. General replay editing, RPG UI expansion, ECS serialization and rollback remain
outside this exercise. Import validation still runs a bounded full verification
before installing a recording; incremental import validation is not a seek API.
The measured worst-case behavior and follow-up assessment are recorded in
[arena replay import responsiveness](replay-import-responsiveness/README.md).

## Example recording and verification

The [mid-dash recording](arena-replay/snapshot-recording.json) starts at gameplay
tick 1 and contains eight subsequent ticks. Try it with:

```sh
cargo run --manifest-path games/arena/Cargo.toml --bin replay -- docs/arena-replay/snapshot-recording.json
```

The [game guide](../games/arena/README.md) owns current acceptance commands.
The [historical replay acceptance report](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/arena-replay.md#verification-evidence)
records the original file-chooser and seek/speed GUI exercises; it does not
establish verification of current code.
