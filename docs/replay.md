# Shared snapshot-backed replay

Titan's replay support separates consumed input and playback progression from a
game's saved state. The RPG and arena use the same engine machinery; each game
still defines what a snapshot means and how to validate and restore it.

## Responsibility boundary

| Layer | Responsibility |
| --- | --- |
| Engine | Bounded recording, digital input frame encoding, recording identity checks, playback cursor and completion result |
| Game | Snapshot schema, validation/restore, fixed-tick simulation, final state and pixel comparison |
| Interactive session | Pause policy, mutation/input isolation, inspection state and monotonic host clock |
| Native/browser host | File selection, keys/buttons, progress display and wall-clock accumulation |

A recording starts with a game snapshot and retains the logical inputs actually
consumed by fixed ticks. It does not record physical key events, pointer gestures
or elapsed wall time. The RPG's movement pulses and the arena's dash edges remain
game rules. The shared file format currently supports digital buttons; the
engine's broader `InputFrame` support for analog values is not an analog replay
format promise.

Games validate imports and run them in a fresh game before changing a live scene.
Playback then restores the starting snapshot, supplies the recorded input frames,
and verifies again at the end. The host pauses exactly at EOF; stepping past the
remaining budget is rejected before advancing. A snapshot resets game-relative
state, while the live session's host frame and inspection identity stay monotonic.

## Engine API

The public `titan::replay` module provides:

- `RecordedButtons::capture` / `decode`: preserve active, pressed and released
  logical actions against a game-supplied action-name schema. Reject unmapped
  actions, analog values, duplicate names and inconsistent edges. Each consumed
  frame is independent, so a snapshot origin may begin with an already-held key.
- `SnapshotRecorder`: start a segment from a non-null game snapshot and host
  frame, append consumed frames up to a fixed cap, invalidate external edits and
  export a v2 `SnapshotRecording` with the game's final state and checksum.
- `SnapshotRecording::parse`: enforce byte/tick limits, expected identity and
  snapshot presence. Games still decode frames and validate snapshot contents.
  Legacy v1 acceptance is an explicit option used by the arena only.
- `Playback`: retain the validated source artifact and expected snapshot, yield
  each frame once, expose remaining ticks/status and accept one completion
  result after the cursor reaches EOF.

The primitives never advance an `App`, restore a world or grant mutation
permission. Install recorded input before the game's fixed simulation; compare
state and pixels afterward. Each game session applies its own pause and input
policy around these operations. The existing in-memory `InputRecording<A>` API
remains available; it is not silently reinterpreted as the portable format.

Diagnostic hosts can call `DiagnosticInspector::record_response` after the
session handles a request. This records its existing response without executing
it again, preserving replay-specific rejection and step limits.

## Two different snapshot models

The arena saves its fixed enemy pool, RNG and dash/contact cooldowns. The RPG
must reconstruct collectible entities that disappear during play, along with
quest progress and shrine activation. Both rebuild derived UI and keep rendering
assets and runtime entity identifiers outside the portable save format.

This second game exercises the shared boundary across entity removal and
recreation, rather than only assigning state to a fixed pool. Exact comparison
uses the canonical game snapshot and software image checksum, excluding the
host frame and entity allocation history.

See [RPG playback](rpg-replay.md) for the shard/shrine scenario and
[arena playback](arena-replay.md) for its file controls and retained v1/v2
compatibility. Scrubbing, speed controls, rollback and a general ECS serializer
remain outside this increment.
