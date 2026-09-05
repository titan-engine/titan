# Titan vision

This document states intended capabilities and design direction, including work
that is not implemented or scheduled. The [design requirements](design-requirements.md)
preserve the opening planning answers and their qualifications; the
[implementation plan](implementation-plan.md) tracks selected execution work.
Undecided choices remain in [open questions](open-questions.md).

## Purpose

Titan exists to make trying game ideas dramatically faster. Its defining
workflow is a human programmer describing a change to an AI agent, followed by
the agent editing the real game source, running it, inspecting the result,
testing it, and iterating with minimal manual intervention.

Titan is intended to combine four roles:

- a collection of composable game-engine libraries;
- an opinionated, convenient high-level game framework;
- a runtime that can be inspected and controlled programmatically; and
- an agent-facing game construction and verification system.

The initial audience is human programmers working with agents. Direct use by
non-programmers and comfortable agent-free development are desirable later,
but neither should compromise the initial agent-assisted workflow.

Titan should eventually be capable of powering good games of many sizes and
genres. Early development will remain grounded in concrete games rather than
trying to implement every anticipated feature in advance.

## Definition of success

Using an AI agent to iterate on a Titan game should feel natural. An agent
should be able to understand a local project, make a change, get precise
feedback, observe both semantic and visual results, and correct its work.

The approach needs rethinking if routine game iteration still depends on a
human translating requests into editor operations, manually examining runtime
state, or relaying opaque failures back to the agent.

## Core principles

### Rust and source code are the initial authoring model

Games are compiled Rust programs. Agents edit the actual source of truth rather
than manipulating an indirect copy through an editor or proprietary database.
Ordinary Cargo workflows remain supported, while the preferred Titan CLI can
provide faster, more structured workflows.

Scenes and other declarative formats may be added when they solve demonstrated
problems. They are not prerequisites for the first usable engine. When multiple
authoring forms exist, each project must have an unambiguous source of truth.

### Agent accessibility is an engine feature

Agent support is not only documentation around a conventional engine. Engine
and game state should be observable and controllable through stable structured
interfaces.

The preferred workflow is:

1. An agent edits Rust source.
2. The agent checks or builds through Cargo or the Titan CLI.
3. The agent runs the game interactively or headlessly.
4. The CLI attaches to an inspection server in the game process.
5. The agent supplies input, advances the simulation, invokes game-defined
   commands, queries state, and captures visual output.
6. The agent evaluates semantic assertions, diagnostics, and screenshots and
   repeats the process.

Every CLI operation should have a stable machine-readable result as well as a
useful human-readable presentation. A project-local agent skill should teach
the CLI workflow. Engine documentation, Rust APIs, and game-local documentation
should make the actual game model discoverable without requiring web access.

Documentation must be designed with limited agent context windows in mind:
local, searchable, precise, and progressively discoverable rather than one
large undifferentiated manual. Runtime diagnostics should also offer actionable
repair suggestions when the engine can identify a useful correction.

### Inspectable and controllable runtime

A running game should expose a public, documented local protocol. Native
inspection uses authenticated loopback HTTP/JSON, with a typed in-code request
model shared with the browser adapter. CLI flags map to that representation.

The intended inspection surface includes:

- registered component and resource types and their available metadata;
- active entities, optional human-readable names, and component values;
- systems, schedules, collision data, diagnostics, timings, logs, and emitted events;
- input injection and deterministic frame advancement;
- screenshots and compact diagnostic bundles;
- explicitly exposed runtime mutations; and
- game-defined commands such as spawning an enemy or loading a level.

Current development policy requires an explicitly enabled mode for tool-driven
mutation. Visibility does not imply writability. The implemented protocol returns
structured rejection reasons such as `read_only`, `mutation_disabled`, `invalid_value`, or
`requires_command`.

An agent should be able to attach to an already-running game automatically.
A human and an agent should be able to interact with and observe the same live
process safely. Arena now demonstrates this workflow; that does not yet make
live-session hosting generic across all games.

### One game, multiple execution modes

The same game code should support:

- an ordinary interactive native build;
- a headless simulation without graphics;
- off-screen rendering for visual verification;
- a browser/WebAssembly build; and
- a browser-based inspector for native or WebAssembly games.

Headless semantic tests must be able to run in CI without a GPU. Interactive
and headless modes may initially be separate invocations of the same code.

### Fast, safe, and performant iteration

Iteration speed, Rust's safety guarantees, and runtime performance are all
important. Titan should pursue all three and measure real tradeoffs rather than
assuming one must always dominate. Compile times and feedback latency are
product-level concerns.

Hot reload is desirable but is not an initial prerequisite. Multithreading
should arrive early after a correct initial execution model exists. Async work
will be useful for asset loading and background tasks but need not precede the
first asset pipeline.

Stable Rust is supported. Nightly-only enhancements may exist but must not be
required for the normal engine. Unsafe Rust must be isolated, minimal, and
accompanied by a clear explanation of the invariants that make each use safe.

## Architecture direction

### Layered API

Low-level systems should be usable as composable libraries. An opinionated
high-level framework should assemble them into an easy default experience.
Major subsystems should be disableable and, where the library boundary makes it
practical, replaceable. The high-level framework may deliberately choose one
preferred integration. The breadth of subsystem replacement remains a design
question; disableability is a firm requirement. APIs should make game behavior
expressible and well documented; a universal preference for constrained APIs
or explicit operations over abstractions has not been chosen.

Crate boundaries should be introduced when responsibilities are understood,
not speculatively for every possible subsystem.

### Custom ECS

Titan uses a custom entity-component-system implementation with Bevy-like
Rust authoring: derived components, typed queries and resources, deferred
structural commands, optional entity names, and customizable schedules. Bevy-like
authoring is the initial direction; other models may be explored later. Optional
human-readable names or paths should coexist with cheap unnamed entities; current
names do not yet provide a general persistent-path system.

The current executor is sequential. Automatic parallel execution where data
access permits is an intended capability, and multithreading should arrive soon
after the correct initial model. Access metadata and deferred boundaries provide
a foundation; a parallel executor is not yet selected implementation work.
Determinism versus maximum parallelism should be configurable. Canonical traversal
is available today for algorithms that need deterministic ordering.

Familiar schedule stages should be defaults in the high-level framework and
remain customizable. The low-level libraries should not impose the framework's
fixed set of stages.

The ECS should remain useful for both very small games and large worlds,
including millions of lightweight entities as a design target, not a measured
capacity guarantee. Save/load and serialization should be considered early in
architecture design, even without format compatibility guarantees. This is
distinct from the intended snapshot
and rollback support for debugging, replay and multiplayer. These capabilities
are not claims about current implementation.

### Game UI

Game UI uses the same entity/component model as the game world. The first
[UI slice](ui.md) provides fixed-pixel text and primary-pointer buttons, exercised
by the arena HUD and RPG quest display. General layout and typography remain
future work. Browser host controls and inspector panels are separate tooling.
Whether reusable gameplay primitives belong in the high-level framework remains
open and does not change the entity-based UI commitment.

### Reflection

Derive-generated reflection metadata is the preferred approach for inspection
and tooling, subject to further design. The current Component derive does not
yet generate complete field reflection. Types that do not participate may remain
opaque. Basic field metadata
can be automatic; descriptions, valid ranges, units, editability, and editor
hints are optional enrichments.

Rust documentation on types and fields should be reusable by generated API
documentation, agent tools, and a future editor. Reflection and serialization
are related capabilities but are not assumed to be identical requirements for
every type.

### Rendering and assets

Titan supports both 2D and 3D in its long-term design, with 2D implemented
first. `wgpu` is the current GPU graphics foundation. Direct native Metal and
Vulkan backends are intended later; their implementation is not scheduled.
High-level rendering APIs should cover normal use while lower-level access
remains possible.

Code-generated meshes, textures, materials, audio, animation, and other assets
should be first-class assets with the same interfaces as file-backed assets.
Code-generated placeholder primitives, textures and simple sounds should make
prototyping possible without external assets. Generation should support build
time, startup and lazy runtime use, with generated assets cacheable on disk.
Constructive solid geometry, including boolean operations, is intended both as
an authoring tool and a runtime capability.

Common external formats such as glTF, PNG, WAV and fonts can use well-maintained,
high-quality libraries; otherwise prefer custom implementations. An engine-native
asset format is also an intended future capability; its design and implementation
are not selected. The engine does not prescribe whether source assets were made
by humans, AI systems or procedural code.

### Determinism and verification

Fixed-step deterministic simulation is a foundational goal. Tests should be
able to advance an exact number of frames, inject or replay input, inspect the
world, and capture output.

Verification can combine ordinary Rust assertions, Titan test helpers and
deterministic input recordings. Image comparisons should offer a configurable
choice between exact pixels and perceptual tolerances. Recordings should support
interactive replay as well as headless verification; the arena now exercises
[both paths with snapshot-backed recordings](arena-replay.md).
The appropriate evidence depends on the feature.

Diagnostic bundles should be produced on failure by default, optionally for
every run, and be disableable. A useful bundle can contain structured errors,
logs, world state, input history, screenshots, and timing information. Tests should support both a
simulation-frame budget and a wall-clock timeout.

## Scope and evolution

Desktop platforms come first, followed by mobile and consoles. Browser support
is unusually important early because browsers provide both a game target and a
surface that agents can interact with effectively.

Multiplayer is intended to support competitive, cooperative and local forms.
Networking, procedural generation, an editor and advanced content systems remain
part of the long-term scope. An editor was never ruled out; it is not an initial
authoring dependency. Concrete games and user needs guide implementation timing
without erasing these intended capabilities.

Backward compatibility is not a current constraint. Games can pin an engine
version. APIs and formats may be redesigned or removed when that improves the
engine, while especially impactful changes should receive concise migration
guides. Current tests and examples must remain valid on the current revision.
Git history records historical decisions; current documentation explains the
architecture as it exists now rather than maintaining a separate decision
diary.

## Project standards

- Titan is initially a private experiment and is intended to become open
  source later under the MIT and Apache-2.0 licenses.
- Dependencies must be compatible with that intended licensing model.
- Prefer custom implementations unless an external crate is universal or is a
  clearly superior, mature, well-maintained choice.
- CI is important from the beginning and will use GitHub Actions.
- Formatting, Clippy, unit tests, headless integration tests, and continuously
  compiling examples should be enforced.
- Architectural checks are intended as part of CI; their specific scope remains
  to be selected.
- Releases should be frequent so games can pin known engine revisions.
- Optimize only in response to evidence, except where an early choice would be
  prohibitively expensive to reverse.
- Initial development may optimize for a modern Apple Silicon MacBook Pro while
  preserving the intended cross-platform architecture.
