# Agent iteration procedure

This small evaluation procedure serves [#95](https://github.com/titan-engine/titan/issues/95)
and the final [adventure](https://github.com/titan-engine/titan/issues/86) and
[factory](https://github.com/titan-engine/titan/issues/93) verification exercises.
It measures whether an agent can edit real source, run it and obtain trustworthy
feedback. It is not a benchmark platform or a CI latency threshold.

## Run one bounded exercise

1. Select an exact revision containing the required game foundation. Record
   engine/game revision, clean/dirty state, OS, CPU, Rust/Python/Node versions,
   target/backend, build profile, cache condition and concurrent work. Read the
   game README, [CLI guide](cli.md) and [runtime skill](../.agents/skills/titan-workflow/SKILL.md).
   Record whether the evaluator already knows the implementation. For the final
   exercises, recruit an unfamiliar agent separately from the implementer.
2. Create a disposable checkout of that revision. Keep changes to game rules and
   scenarios there, preserve its patch and fixture, and leave current gameplay
   untouched. Use unique instance names and explicit project selection. Keep
   caches separate or record contention. Coordinate native/browser focus with
   other users before launching a window.
3. State the task and its expected result *before* editing. Start a monotonic
   timer at task handoff, including reading, searching, editing, failed attempts,
   builds and verification. Also time command phases separately. If only command
   phases are measured, label them as such; do not call them full authoring time.
   Report preparation, scheduler waits and GUI coordination separately. One run
   is a sample, not a percentile or a stable performance ranking.
4. Build, launch with a wall-time bound, discover the selected instance, inspect
   capabilities/commands/queries/entities, perform the task, inspect exact state,
   replay from a fresh state and capture where supported. Assert semantic state
   and rejection codes, not merely CLI exit status. Stop the timer only when the
   expected result and relevant evidence are verified. Save all failed attempts,
   corrections, missing documentation and human/agent interventions.
5. Diagnose one intentional invalid operation: save the error, inspect its
   diagnostic bundle if supplied, query state to check side effects, correct the
   cause and verify recovery. Separate this expected rejection from accidental
   authoring/harness failures. A timeout is not proof that mutation was cancelled.
6. Stop owned processes, verify discovery cleanup and preserve sanitized evidence.
   Never publish registry tokens, raw registry files or private machine/session
   paths. Save capture identity beside its image; retain the exact scenario,
   patch, assertions and commands needed for another agent to repeat the result.

Use the existing [acceptance deadlines](acceptance-timeouts.md): builds and
runtime commands have separate hang bounds, not success-speed requirements.
There is no hot reload assumption: rebuild and relaunch changed source.

## Representative skeleton tasks

| Task | Adventure foundation | Factory foundation |
| --- | --- | --- |
| Change a rule | Change a movement increment in a scratch variant; assert displacement and inactive-character stationarity. | Change the maximum advance request in a scratch variant; assert its accepted/rejected boundary. |
| Construct a scenario | Submit a short fixed input route involving both characters; inspect positions and selected character. | Place an extractor, conveyors and processor; inspect orientation and ports, then advance bounded ticks. |
| Diagnose a failure | Submit an invalid/unsupported operation; read the structured error and verify state/recovery. | Try invalid placement or fixed-delivery mutation; inspect rejection, unchanged state and a corrected construction. |

These tasks exercise authoring and feedback without requiring jumping, puzzles,
transport, recipes or completion. Final exercises substitute their actual
approved puzzle/machine variation and solution/failure routes while retaining
this reporting format. #93 additionally needs repeated larger-fixture samples
and workload counts; keep semantic assertions separate from timing. Neither
final exercise may substitute these skeleton results for its own gameplay,
independent authoring or native/browser visual verification.

## Workflow and capability boundaries

Commands below run from the repository root unless a game guide says otherwise.
Use the package's Cargo target directory for its binaries, and the root target
for `titan`; `CARGO_TARGET_DIR` can change both. Start with:

```sh
cargo build -p titan-cli
cargo build --manifest-path games/adventure/Cargo.toml --bin titan-adventure
cargo build --manifest-path games/factory/Cargo.toml --bin titan-factory
# Two separate bounded server invocations, each in its own terminal:
games/adventure/target/debug/titan-adventure --serve --project games/adventure --instance baseline-adventure --run-for-ms 120000
games/factory/target/debug/titan-factory --serve --project games/factory --instance baseline-factory --allow-mutation --run-for-ms 120000
# Repeat these reads with each matching project and instance:
target/debug/titan --format json --project games/adventure --instance baseline-adventure instances
target/debug/titan --format json --project games/adventure --instance baseline-adventure capabilities
target/debug/titan --format json --project games/adventure --instance baseline-adventure commands
target/debug/titan --format json --project games/adventure --instance baseline-adventure queries
target/debug/titan --format json --project games/adventure --instance baseline-adventure entities
target/debug/titan --format json --project games/adventure --instance baseline-adventure query state
```

The [adventure guide](../games/adventure/README.md#inspect-and-control) documents
complete input snapshots, exact stepping, switch/restart, recording and native
player control opt-in. `query recording` returns a response envelope; extract
its `response.value` before supplying `{"recording": ...}` to the CPU `replay`
command. The actual native player uses `load_replay` followed by stepping or
resuming playback; discover each host's command metadata.
CPU hosts have no 3D capture. Native GPU capture/replay is reproducible with
`python3 games/adventure/scripts/test-player.py`; it checks accepted frame,
revision and session generation without advancing the paused scene.

The [factory guide](../games/factory/README.md#native-control) documents
`place`, `construct`, `sequence`, state/tile queries and recording. Replay here
means applying the same ordered construction operations/advances from a fresh
state; there is no interactive recording-playback UI. The headless capture is
a software image, not proof that the native GPU player was inspected. Construction
can succeed with disconnected outputs; time advancement in the foundation does
not create items or deliveries. Rejected sequence operations may coexist with
process exit zero; inspect every outcome.

Both games expose read-only component fields and use commands/input for changes.
Native discovery needs matching project identity. Browser adapters use their
local dispatch API, not native discovery. Build with each game's
`scripts/build-browser.py` and run its `scripts/test-browser.mjs` for actual
WASM CPU evidence. Serve its `web/` directory and run `/play/test.html` in an
actual browser for GPU evidence; adventure has explicit WebGPU/WebGL2 query
parameters. Node success never implies browser graphics success. Record any
unexercised path as unmeasured, and unsupported paths as unsupported.

## Evidence record

Copy [the record template](evidence/agent-iteration/template.json) once per task
or phase. It is an example format, not a new runtime protocol. Use null for an
unmeasured duration, never zero; report failures even if the final attempt passes.
Keep the raw bounded command results locally and publish a sanitized result
summary plus small fixtures/patches/images needed to audit the assertions.

The [initial measurements](evidence/agent-iteration/README.md) are observations
at one pinned revision. They establish a starting point, not portable performance
or a finished-game acceptance claim.
