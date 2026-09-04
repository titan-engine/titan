# First milestone: agent-built procedural 2D RPG slice

## Goal

Prove Titan's central workflow with a small procedural 2D RPG built in Rust.
Given a broad prompt, an agent should be able to create and improve the game,
run the same source interactively and headlessly, inspect it, control it, and
produce evidence that its changes work.

This milestone is a workflow proof, not a claim that every future engine system
is complete.

## Representative user requests

- "Get started on a game that is a 2D procedural RPG with generated pixel art."
- "Make the generated pixel art prettier."

The first request tests broad implementation and integration. The second tests
whether the visual feedback loop is good enough for an agent to make a
subjective improvement rather than merely compile code.

## Player-visible result

The exact design remains deliberately flexible, but the slice should include:

- a deterministic, seed-generated 2D area;
- a controllable player;
- a camera and a minimal readable presentation;
- code-generated pixel-art assets;
- at least one simple RPG interaction so this is more than a movement demo; and
- a native playable build and a browser/WebAssembly playable build produced
  from the same game source.

The specific RPG interaction and initial rendering feature set are open until
we choose the smallest slice that meaningfully exercises state, input, and
verification.

## Agent-visible result

An agent working only from the repository should be able to:

1. discover how to build, run, inspect, and test the project;
2. edit ordinary Rust game code;
3. check the project through Cargo or a Titan CLI command;
4. launch a native, browser, or headless run;
5. attach the CLI to the running game;
6. discover exposed entities and components;
7. inject input and advance deterministic simulation;
8. invoke at least one game-defined command;
9. mutate an explicitly writable value when development mutation is enabled;
10. capture a rendered frame;
11. run semantic assertions against game state; and
12. receive a structured diagnostic bundle when something fails.

All CLI operations in the milestone must support stable structured output and a
human-readable rendering of the same result. Exact compatibility across future
Titan versions is not required; stability applies within the pinned version.

## Required engine slice

### Application framework

- A small Bevy-like `App` API.
- Startup, fixed-update, update, and rendering integration sufficient for the
  demo, exposed as customizable high-level defaults rather than hard-coded ECS
  rules.
- Native interactive, headless simulation, off-screen rendering, and WebAssembly
  launch paths using the same game definition.

### ECS

- Entities and typed Rust components.
- Component derive and minimal reflection metadata.
- Typed system queries and resources.
- Command-buffered structural changes.
- Optional entity names or paths.
- A deterministic single-threaded schedule first, without preventing an early
  multithreaded scheduler.

### Rendering and procedural assets

- The smallest 2D rendering layer needed for the RPG slice.
- Code-generated pixel-art textures or sprite data.
- Deterministic capture of rendered output.
- A GPU-independent CI path. This can use a reference/software rendering path
  or another implementation selected during technical prototyping.

### Input and replay

- Keyboard input for interactive play.
- Programmatic input injection.
- A deterministic recording format that can be saved and replayed both in tests
  and interactively.

### Inspection server and CLI

- Automatic discovery of a local running game.
- A typed protocol with a documented structured wire representation.
- World and entity inspection.
- Explicitly enabled development mutation.
- Structured reasons for rejected mutations.
- Game-defined command discovery and invocation.
- Frame advancement and input injection in controlled runs.
- Screenshot capture.
- Human-readable and structured CLI output.

The initial command names are intentionally not frozen. Candidate workflows
include `titan check`, `titan play`, `titan test`, `titan attach`,
`titan inspect`, `titan input`, and `titan capture`.

### Testing and diagnostics

- Ordinary Rust test integration with Titan-specific helpers.
- Exact fixed-frame advancement.
- Semantic queries and assertions.
- Exact and tolerance-based screenshot comparison where appropriate.
- Frame-budget and wall-clock timeouts.
- Failure bundles containing enough structured context for an agent to diagnose
  the problem without a human relaying information.

### Local documentation

- A compact project-local agent skill focused on the Titan CLI workflow.
- Searchable engine API documentation available without network access.
- A small example game that always compiles against the current engine.

## Acceptance scenario

The milestone is complete when a capable coding agent, starting with the broad
RPG prompt and only repository-local guidance, can produce the playable slice
and complete a loop resembling this:

```text
edit source
    -> check/build
    -> launch controlled game
    -> replay input and advance fixed frames
    -> inspect semantic state
    -> capture rendered output
    -> diagnose or improve
    -> repeat
```

A human can then play the resulting native or browser build from the same game
source. The workflow must be demonstrated by automated tests and by at least
one recorded end-to-end agent iteration, including a visual improvement.

## Explicitly not required for this milestone

- 3D rendering;
- a general-purpose editor;
- hot code reload;
- multiplayer or rollback networking;
- mobile or console builds;
- a complete asset import pipeline;
- a production-ready multithreaded scheduler;
- a stable long-term scene format;
- comprehensive RPG mechanics; or
- compatibility with earlier Titan APIs or formats.

These exclusions define sequencing, not permanent non-goals.

