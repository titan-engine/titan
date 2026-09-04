# Implementation plan

This is the current execution plan for reaching Titan's first milestone. It is
a living roadmap, not an architectural history. Update it when priorities or
the current implementation change; Git retains earlier versions.

## Current foundation

Titan currently has:

- a custom ECS with generational entities and sparse-set component storage;
- component derives, basic metadata, optional names, and two-component joins;
- deferred structural commands applied at deterministic schedule boundaries;
- ordered application schedules, plugins, resources, and fixed-tick execution;
- renderer-neutral 2D frames and an exact GPU-free software renderer;
- deterministic logical input frames and replay recordings;
- a procedural 2D RPG example with generated pixel art, a small shard quest,
  semantic assertions, and an exact rendered-image checksum;
- transport-neutral structured inspection request and response types;
- in-process capabilities, status, entity inspection, controlled stepping, typed
  game commands, logical input injection, and software capture hooks;
- a CLI with human-readable and JSON output for checking, testing, and running
  examples; and
- authenticated native loopback transport, project-local discovery, and CLI
  attachment to the paused headless RPG; and
- native and WebAssembly CI checks, including a separate-process control loop.

## Next vertical objective

Prove one complete agent-facing control loop against the procedural RPG:

```text
launch headless game
    -> discover and attach
    -> inspect capabilities and named entities
    -> inject or replay input
    -> advance exact fixed ticks
    -> invoke a game-defined command
    -> capture a software-rendered frame
    -> return structured results and artifacts
```

This objective takes priority over broadening the renderer or adding more game
features. It directly tests Titan's defining workflow.

## Phase 1: complete in-process inspection

Completed: the procedural RPG acceptance test now exercises this sequence
through protocol requests, including the original exact capture checksum.
See [in-process inspection](inspection.md) for adapter and failure semantics.

- Register command metadata and typed handlers.
- List commands deterministically.
- Invoke commands with structured arguments at the inspection safe point.
- Increment state revision only after successful mutations or commands.
- Add an RPG command such as resetting the quest or spawning a shard.
- Surface deferred ECS command failures through structured protocol errors.
- Register logical input injection and software-frame capture hooks.
- Test the entire request sequence without sockets or subprocesses first.

Completion signal: an in-process test can inspect the RPG, replay its route,
activate the shrine, and retrieve a capture result using protocol requests only.

## Phase 2: native discovery, transport, and CLI attachment

Completed for macOS and Linux. `python3 scripts/test-control-loop.py` drives
the headless RPG through separate CLI processes and verifies replay, inspection,
command invocation, exact capture, structured failures, and clean shutdown.

- Put transport requests into a bounded queue; transport code must never access
  the ECS world directly.
- Drain requests only at explicit deterministic safe points.
- Add a loopback-only native HTTP/JSON adapter.
- Allocate an ephemeral port and a random bearer token for each run.
- Write owner-only per-instance registration files containing project identity,
  process identity, endpoint, protocol version, mode, and token.
- Ignore or clean stale registrations and report ambiguous matches explicitly.
- Add CLI commands for capabilities, status, entities, stepping, input,
  invocation, and capture.
- Keep JSON stdout to exactly one structured response; send progress to stderr.
- Test discovery, authentication, schema mismatch, timeouts, full queues, and
  clean runtime shutdown.

Completion signal: one terminal launches the headless RPG and another can
discover it and complete the vertical control loop using the `titan` CLI.

## Phase 3: browser inspection bridge

Completed: the shared RPG runs through the WASM protocol adapter with explicit
control opt-in, PNG capture, and a capability-driven browser inspector. The
actual WASM control loop and same-origin bridge checks run in CI; the browser
UI has also been exercised through the reference route.

- Expose the same typed protocol handler through a WASM/in-page message bridge.
- Build a minimal browser inspector that discovers capabilities rather than
  assuming them.
- Keep browser mutation explicitly enabled and same-origin by default.
- Add an optional outgoing connection to a Titan development host only when a
  browser game must be controlled by the native CLI.

Completion signal: the protocol-level acceptance sequence works against a WASM
game without changing its request or response model.

## Phase 4: interactive rendering

Current task: add immutable extracted snapshots, a real GPU sprite pipeline,
and native/browser interactive runners around the shared game.

- Add an immutable, deterministic render-extraction boundary after fixed ticks.
- Implement a `wgpu` 2D backend consuming the same frames and CPU image assets
  as the software renderer.
- Add a `winit` native runner without coupling its event loop to `App`.
- Add a WebAssembly canvas runner for the same game builder.
- Map native and browser events to logical actions at fixed-tick boundaries.
- Treat software captures as exact reference output and GPU captures as
  integration evidence with configurable perceptual tolerance.

Completion signal: the procedural RPG is playable natively and in a browser,
while its headless replay continues producing the same semantic result and
reference image.

## Phase 5: ECS authoring ergonomics

- Add typed `Query`, `Res`, `ResMut`, and `Commands` system parameters.
- Detect conflicting component and resource access before systems execute.
- Generate tuple queries and system parameters to a practical fixed arity.
- Keep the initial executor sequential while recording access metadata needed
  by a future parallel scheduler.
- Add an explicit `apply_deferred` schedule node.
- Add bundles so spawning a normal game entity is concise.
- Add a deterministic sorted-by-entity iteration option for algorithms that
  require canonical ordering.

Completion signal: the RPG uses the intended Bevy-like public API without
requiring systems to manually accept and navigate `&mut World`.

## Phase 6: diagnostics and agent documentation

- Produce a diagnostic bundle on failure by default, configurable to always.
- Include structured errors, relevant world state, input history, fixed tick,
  state revision, logs, timings, and captures.
- Generate compact local API summaries from component and command metadata.
- Add a project-local agent skill focused on the Titan CLI workflow.
- Add frame-budget and wall-clock timeout support to controlled tests.
- Add exact and perceptual image comparison helpers.

Completion signal: an agent can diagnose a failed feature attempt using only
repository-local documentation and the generated diagnostic bundle.

## Deliberately deferred

Do not expand into these areas until the vertical objective works:

- 3D rendering;
- a general-purpose editor;
- hot code reload;
- multiplayer and rollback networking;
- multithreaded system execution;
- mobile and console targets;
- a general scene format;
- a complete imported-asset pipeline; and
- broad RPG mechanics.

The architecture should leave room for these features, but speculative support
must not slow down validation of the agent iteration loop.

## Ongoing quality gate

Every increment should leave the following green:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check -p titan -p titan-protocol -p titan-browser --target wasm32-unknown-unknown
```

Examples are tested against the current engine revision. Breaking APIs and
formats are acceptable during this phase, but the current repository must
remain internally consistent and especially impactful changes should include a
concise migration note.
