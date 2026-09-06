# First sound exercise: RPG shard pickup

This is the design proposal for [issue #37](https://github.com/titan-engine/titan/issues/37),
not a shipped audio API or approval to implement it. It selects a bounded exercise
for the simple generated-sound and shared generated/file-backed interface
requirements (R1.50 and R2.57–59 in [design requirements](design-requirements.md)).
Implementation selection remains on the issue board; this document describes
the proposed contract, not a second task backlog.

## Cue and existing integration points

Recommend a short, quiet rising pickup tone when the RPG collects one or more
shards in a fixed tick. Emit **one cue per collection tick**, even when several
shards overlap; include the number collected as semantic evidence. Moving,
spawning a shard, opening the journal, loading a save and activating the shrine
alone produce no cue. This avoids adding gameplay rules or a second sound.

The current [`collect_shards`](../fixtures/rpg/src/lib.rs) system
increments quest progress and queues shard despawns in canonical traversal order.
It emits no audio event today. A future game-owned resource would accumulate
that tick's pickup count there and expose the cue after the fixed schedule's
deferred commands have completed successfully. Presentation consumes it once
per fixed tick, never by comparing rendered HUD text or polling total progress.
The [reference route](rpg-replay.md#acceptance) collects at route ticks 2, 5 and 9;
tick 11 reaches the final reference state. Existing gameplay and software checksum
`f7a298f62ad75c1c` must remain unchanged.

Use a startup-generated mono clip: 48,000 samples/second, 120 ms (5,760 signed
16-bit samples), a triangle tone rising from 660 to 990 Hz, peak amplitude at
most 0.15 full scale, with a 5 ms attack and 20 ms release ending at zero.
An explicitly versioned integer phase accumulator and integer envelope with
specified rounding should make source PCM identical on native and WASM. No
randomness, wall time, platform sine implementation or output-device rate enters
generation. Freeze exact arithmetic and a PCM digest in implementation tests;
this proposal does not invent an unmeasured checksum or claim the tone sounds good.

## Small shared boundary

The names below describe responsibilities, not committed public Rust signatures.
Start with game/example support and host adapters; introduce an engine crate or
public API only if the approved implementation demonstrates the need.

| Value/operation | Proposed contract |
| --- | --- |
| `SoundClip` | Immutable owned mono signed-16 PCM at 48 kHz; nonempty and at most 24,000 samples (500 ms). Validate once before installing. |
| Generated or WAV constructor | Both return the same validated `SoundClip`; provenance is diagnostic metadata, not a different playback path. |
| `SoundCue` | `pickup_v1`, recording-relative tick, ordinal within tick (always zero here), and positive `collected_count`. No entity handles, device times or sample data. |
| `present(cue)` | Nonblocking best-effort request referencing the retained clip. Record semantic intent before consulting output state. |
| `stop()` | Idempotently discard pending presentation and stop the current voice; do not erase semantic evidence. |
| `status()` | Bounded host diagnostics: disabled, awaiting activation, ready, unavailable or closed; reason and dropped-request count. Ready means submission is possible, not proof of audibility. |

The host owns one retained clip, at most one active voice and one pending request.
A newer request replaces the pending one. A cue arriving during playback replaces
the current voice with a short bounded fade (up to 5 ms) before starting the new
clip; no simultaneous mixing or unbounded queue. Submission and callback failures
become diagnostics, never simulation errors. The audio callback must not access
the ECS, allocate, block on the game thread, or perform file I/O. A null sink
requires neither native device libraries nor browser audio initialization.

## Host lifetime and activation

Native playback belongs to the interactive RPG host, not `App` or the headless
controlled runner. Recommend evaluating CPAL as the thin device adapter during
implementation: its [host API](https://docs.rs/cpal/latest/cpal/traits/trait.HostTrait.html)
allows no default output device, and [stream construction](https://docs.rs/cpal/latest/cpal/)
requires format/configuration selection and error handling. It is a candidate,
not an added or pinned dependency. Check licensing, native build dependencies
and supported macOS/Linux configurations before selecting a version. A higher-level
player library is an alternative if it materially reduces single-voice lifecycle
work; direct platform APIs would duplicate the platform integration too early.

Open the default device once on native interactive startup. Adapt the source PCM
to a supported device rate/channel/sample format outside simulation (bounded
resampling and mono duplication); test at least 44.1 and 48 kHz. Unsupported
configuration, absent device or stream failure enters unavailable with an
explanation and continues silently. Do not busy-retry or accumulate old cues.
An explicit local enable/retry action may reopen output; mute closes/stops it.
Headless and verification runs select the null sink before any device discovery.

The browser host owns one `AudioContext` and one reusable `AudioBuffer`. Add a
local **Enable sound** control; create/resume output directly in its trusted
activation handler, before any awaited work. It must not require inspection
control permission. Injected game input and inspector messages do not unlock
sound. The [Web Audio specification](https://www.w3.org/TR/webaudio-1.0/#dom-audiocontext-resume)
allows resume to remain pending while starting is disallowed: do not block game
startup or equate a requested resume with a running context. Report awaiting
activation until running, expose failures, and allow a later local retry.
Reuse PCM storage but create a new
[`AudioBufferSourceNode`](https://www.w3.org/TR/webaudio-1.0/#AudioBufferSourceNode)
for each play; release finished/stopped nodes. Browser resampling and device
latency are presentation behavior, outside the deterministic contract.

While disabled, suspended or unavailable, discard presentation requests at once;
activation never plays a backlog. Loss of a running browser context returns to
awaiting activation (or unavailable on failure), stops the voice, and requires
local retry. A running context cannot prove speakers exist, are unmuted or are
audible; report only observable API state. Do not request microphone access or
probe physical audibility.

| Transition | Presentation policy |
| --- | --- |
| Pause, focus loss or hidden page | Stop voice and clear pending request; resume admits only new cues. |
| Explicit single-step while paused | Record semantic cue but suppress playback. |
| Normal live play or real-time imported replay | Submit each new tick's cue when output is ready. |
| Multi-tick inspection stepping, reference fast-forward or replay validation | Record every semantic cue; suppress playback to avoid a burst. |
| Restart, successful save load, replay restart/exit | Stop and clear presentation; reset the event segment/cursor, retain clip and output preference. Restoring progress itself emits nothing. |
| Failed save/recording validation | Leave game and event segment unchanged; no cue. |
| Host shutdown/page teardown | Stop voice, release clip, close context/stream, ignore late callbacks using a host generation token. |

Mute/enable and output-state changes are local presentation controls and must not
change the game snapshot, input recording, RNG or fixed-step timing. Any pending
activation result must also be invalidated by mute or teardown so it cannot
unexpectedly re-enable output.

## Device-free event evidence

Keep sound intent separate from output receipts. For this exercise, add a bounded
game-owned trace exposed by a read-only game query, not a general engine event
bus. Retain the latest 256 cues plus total count and an explicit truncation flag.
A headless harness drains each step when it needs a complete longer trace;
truncation must never masquerade as complete evidence. Output diagnostics have
separate bounds and are excluded from deterministic comparisons.

A segment begins at fresh game creation or a successful load/restart. Ticks are
one-based fixed updates relative to that origin, so a native/browser replay of
the same snapshot and inputs produces the same cue trace even if host frames
differ. Reset the presentation consumption cursor on a new segment; an additional
host-local segment generation prevents old asynchronous work crossing resets.
This generation is not part of the canonical trace.

The input recording remains the source of replay. Derive sound cues again from
simulation; do not inject saved sound requests and risk double playback. Keep
PCM, device state and voices out of save data. Export the cue trace as separate
versioned acceptance evidence alongside the recording, with cue/generator
version, PCM digest and segment origin. Existing recording schemas and their
pixel verification need not change for this first exercise.

## WAV parity decision

Recommend including one tiny file-backed counterpart in the eventual exercise:
a loose WAV containing exactly the generated cue's PCM, authored by a reproducible
fixture script. Startup generation remains the default and needs no audio file.
An explicit fixture selection loads WAV bytes natively or fetches them in the
browser, decodes to `SoundClip`, then uses the identical host path. Both forms
must be selectable for the same audible and headless route. Package the fixture
in the native development bundle and browser build, following the existing
[loose image exercise](assets.md). No runtime filesystem cache is warranted.

Bound the fixture to 64 KiB and the clip limit above; accept only RIFF/WAVE PCM,
mono, 16-bit, 48 kHz. Validate chunk lengths/padding, alignment and nonempty data
before allocation, reject truncated/oversized/unsupported data, and test bounded
unknown chunks. Prefer a reviewed decoder if it fits this narrow contract; do not
turn this into a general format implementation. An explicit file selection that
fails reports unavailable with its asset error and continues silently without
silently substituting generated sound. The game remains usable.

Generated-only playback is smaller but would not prove file-backed parity. An
in-memory WAV round trip tests decoding but misses loose-file delivery and
packaging. The tiny fixture is therefore recommended; arbitrary WAV formats,
compressed formats, hot reload, asset identity/dependencies and a general asset
manager remain outside this exercise, as do music, spatial audio, streaming and
a mixer.

## Verification required before an audio implementation can be accepted

These are planned checks, not results from this documentation investigation.
Use the existing [runtime workflow](../.agents/skills/titan-workflow/SKILL.md)
and [quality gates](verification.md); retain the three required PR jobs.

| Check | Required evidence |
| --- | --- |
| Native and actual WASM, null sink | Reference route emits `pickup_v1` at ticks 2, 5, 9, count 1 each; no cue at startup or idle ticks; final state and RGBA checksum unchanged. Exact traces match with output disabled, unavailable and fake-ready. |
| Collection edge cases | Overlapping shards yield one cue with their count; next tick does not repeat it. Spawning/loading progress alone is silent. |
| Snapshot/replay | Origin after first pickup yields only remaining pickups at relative ticks 3 and 7; live and replay traces match. Restart repeats the segment once; failed loads preserve it; bulk stepping and validation never submit audible requests. |
| Clip and WAV | Native/WASM source PCM digests match; length, envelope endpoints and peak bound hold; decoded fixture equals generated samples exactly. Invalid inputs fail within memory/size bounds; browser missing fetch and native missing file remain playable. |
| Fake output lifecycle | Cover pending activation, rejection, no device, stream loss, replacement, mute, pause, hidden page, retry, teardown and late callbacks; prove no stale playback or unbounded retention. A trace longer than 256 explicitly reports truncation. |
| Browser integration | Actual WASM route before activation stays playable and records intent; trusted Enable sound action transitions only after context runs. Exercise suspension/retry and clean teardown. Mocked unit tests alone do not establish browser activation behavior. |
| Native audible check | On the reference macOS desktop, record OS/device, revision and selected clip source; manually collect each shard with enough spacing to hear each cue, pause/restart, mute/retry and exit. Repeat for generated and WAV sources. Report audibility, clicks/clipping and any limits. |
| Browser audible check | Record browser/version/OS and device; reload before activation, enable via local control, then collect spaced pickups for both sources. Verify no old cues on enable, mute or focus return. |

Required CI evidence is semantic and device-free: no microphone recording,
loopback audio hardware or exact rendered waveform comparison. Audible checks
need an actual listener; API success or a screenshot is insufficient. Record a
missing audible check as unverified, never as passed. Any backend choice that
cannot meet these bounds returns to design review before expanding the scope.
