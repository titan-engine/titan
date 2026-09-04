# Milestone 2: build a second game from the starter

## Goal

Prove that Titan's agent workflow transfers to a new game without relying on
RPG-specific setup or knowledge. Deliver a minimal reusable starter and use it
to build a tiny arena-survival demo through repository-local guidance.

The accepted RPG stays as an independent regression example. This milestone
measures reuse and discoverability, not how many engine features can be added.
The [implementation plan](implementation-plan.md) defines execution order.

## Game brief

Build a small top-down arena-survival game with generated 2D art. The player
moves within a fixed arena while enemies spawn deterministically and pursue
them. Contact reduces health. Surviving a configured fixed duration wins;
losing all health ends the run. Show health, elapsed time, and the outcome, and
provide a restart operation.

Use a single arena, one enemy behavior, simple collision rules, and a small
readable presentation. Keep weapons, progression, multiple levels, scrolling,
and elaborate physics out of the initial slice. Choose tuning values through
a short playable iteration, then pin the seed and replay inputs used by tests.

The same game definition must run headlessly, in a native window, and in a
browser. Simulation rules must not depend on a graphical host or wall-clock
rendering rate.

## Starter outcome

A developer or agent can copy the starter into a separate directory, configure
its documented Titan dependency, and build it without importing RPG support
modules. It provides a minimal scene and the necessary native, browser, and
controlled-run entry points, with an obvious place to put game code.

The starter exposes the current inspection workflow and safe defaults. Its
small amount of sample content must be replaceable rather than an RPG fork.
Public engine/host code may be reused; shard rules, shrine state, RPG input
adapters, and reference assets may not be hidden dependencies.

## Independent-agent exercise

A fresh agent receives this brief, the starter, and local documentation. It
builds the arena game, runs it, inspects failures, and iterates. Record any need
to inspect undocumented internals or receive extra guidance. Fix demonstrated
problems in the appropriate layer, then verify the corrected workflow with a
fresh agent.

Retain a compact record of the final reproducible commands, important diagnosed
failures, their fixes, and verification artifacts. Avoid a chronological dump;
Git holds implementation history.

## Acceptance

- The copied starter builds and runs using only documented setup.
- Arena game code is independent from RPG code and compiles against the current
  engine revision.
- Native and browser builds are playable from the same game definition, with
  visible health, timer, outcome, and restart behavior.
- A headless test drives deterministic input for exact ticks and verifies enemy
  spawning, contact damage, loss, survival, and restart semantics. Use focused
  recordings or scenarios rather than relying on a single happy path.
- The CLI discovers and inspects the arena runtime, injects input, steps it,
  invokes restart, edits an explicitly validated development field, and captures
  a rendered result. Disabled or invalid mutations return structured errors.
- A failed attempt produces a useful bounded diagnostic bundle containing the
  relevant game state and recent input; an agent uses it to diagnose the issue.
- Captures and semantic assertions verify the final game. Intentional visual
  changes are reviewed before updating image expectations.
- The independent-agent verification succeeds through local guidance, and the
  user reviews the playable result.
- CI covers both games and starter setup while preserving the accepted RPG
  replay and capture checks.

## Scope boundaries

Add engine features only in response to the starter or arena exercise. Simple
collision logic can begin in the game. A fixed arena does not require a scrolling
camera, a physics dependency, or a new scene format. A generator CLI, broad
reflection, parallel scheduling, and asset import are not prerequisites.

Continue using the existing native diagnostic integration and browser control
model. Automatic browser bundle export and new platform support are separate
work unless they prove necessary for this exercise. Resolve concrete design
choices through the [open questions](open-questions.md), not speculative systems.
