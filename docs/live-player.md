# Live-player diagnosis verification

Input consolidation and actual-player inspection are implemented and locally
verified. The arena's native and browser players now render, query, capture and
record one `ArenaSession`. No separate simulation is used to inspect the player.
The existing isolated inspector remains available for headless exercises.

## What this exercise exposed

- Buffered input cancellation had diverged across copied browser hosts. A real
  RPG tap/blur regression was reproduced and fixed with shared button accumulation
  and browser source/cancellation handling. See [input boundaries](input.md).
- Component fields could not express a read-only query of game resources or an
  input recording. Registered read-only queries now provide that bounded surface.
- An owning browser inspection wrapper could not borrow the playable app. Shared
  request policy now supports caller-owned apps, with explicit host clock state
  and revision accounting for local changes.
- Held input alone cannot reproduce a dash release/repress between ticks. Arena
  records the actual consumed values and press/release edges after source selection.

Games still own bindings, pulse cadence, dash behavior, session composition,
recording format and reset policy. No application framework, snapshot/rollback
system, new scene format or difficulty setting was introduced.

## Demonstrated diagnosis

The native GPU acceptance test played until contact, paused the actual window,
queried its state and exported a recording. The retained run has 192 ticks from
host frame 16, health 2 and checksum `ae923e36040921f9`. A new native headless process
verified its complete state and software image. Run length varies slightly with
when the window receives the pause request; the retained artifact is exact.

Computer Use independently exercised the actual browser player. An idle run
lost at game tick 310; the host was paused at frame 944. Read-only state inspection
and capture both reported frame 944, checksum `bade431583926480`. Verify & export
replayed all 944 consumed ticks successfully in WASM. The downloaded recording
was then verified by the native headless replay binary with the same state and
checksum. Enabling controls preserved that game; one step advanced to 945.
Disabling controls and restarting preserved host frame 945 and reset health 3,
player position and recording length to zero.

The actual GPU browser fixture also passed read-only denial, same-presented-frame
inspection, in-place control opt-in, pause/resume timing transitions, exact step,
capture and recording replay (two-tick checksum `4e781eb853c34dae`). Its initial
failure incorrectly expected a permission toggle to change the clock epoch; the
fixture now tests actual clock transitions. A stale browser test double was also
updated to support the new panel while retaining input cancellation assertions.

Retained reproducible recordings:

```sh
cargo run --manifest-path games/arena/Cargo.toml --bin replay -- docs/live-player/native-contact.json
cargo run --manifest-path games/arena/Cargo.toml --bin replay -- docs/live-player/browser-loss.json
```

The [arena README](../games/arena/README.md) gives native attachment and browser
panel instructions. Native `--inspect` is read-only; `--allow-control` permits
remote controls. Browser control opt-in modifies the existing session. Ordinary
local Pause/Resume and restart remain player actions.

## Verification and limits

All 21 standard checks plus actual native GPU attachment and GPU pixel comparisons
passed. [Compact command results](live-player/verification.json) retain 23 checks.
This includes existing RPG/starter/arena native and actual-WASM loops, copied
starter, relocated macOS bundles, shared browser input tests, and the new live
session tests. Independent reviews of input, session/replay, host adapters and
shared query policy found no unresolved actionable issues. The macOS CI job now
runs the actual-player acceptance script; these commits have not been pushed,
so remote CI has not run for this increment.

Recordings retain at most 3,600 ticks from restart and reject files over 2 MiB.
Truncation and out-of-band position edits are explicit invalidation conditions,
not successful exact replays. Replay validates format, seed, action schema,
frame bounds and final state/image. This is deterministic replay from restart,
not arbitrary rewind. Native and browser live support is currently demonstrated
in arena; RPG and starter retain their isolated inspection hosts.

RPG checksum `190a92085def5677` and arena initial `e096abf94fd12c24` / winning
`b5cf61da6f50efd7` remain unchanged. Dash tuning is unchanged.
