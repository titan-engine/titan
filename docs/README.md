# Titan documentation

Start with what you want to do. Titan is early in development; the
[README](../README.md#what-works-today) describes what is supported today.

The README owns supported capabilities and getting started; this index owns
navigation. Reference guides own API contracts and usage, and each game directory
owns its game rules and controls. [Verification](verification.md) owns quality
gates; [workflow](workflow.md) owns contribution and integration policy. Issues
own pending work and approval status. Historical observations describe only the revisions they measured.

## Try Titan

- [Play the collection room](../games/collection-room/README.md) — Titan's first
  small 3D demo, with hosted, native and local-browser instructions.
- [Run the RPG](../README.md#try-the-demo) — native, headless, and browser setup.
- [Play the arena](../games/arena/README.md) — a standalone survival game.
- [Inspect and control a game](cli.md) — discovery, commands, fields, and captures.
- [Record and replay](replay.md) — shared concepts, with [RPG](rpg-replay.md)
  and [arena](arena-replay.md) controls.

## Build a game

- [Copy the starter and make your first edit](../starters/minimal/README.md).
- [Author ECS components and systems](ecs-authoring.md).
- [Render a game](rendering.md) and [add entity-based UI](ui.md).
- [Load assets](assets.md) and [generate cached images](generated-assets.md).
- [Save and load game state](save-load.md).
- [Use inspection in-process](inspection.md) or [in the browser](browser.md).
- [Reuse native and browser host tooling](host-tooling.md).

## Understand the project

- [Vision and principles](vision.md) — intended direction and tradeoffs.
- [Design requirements](design-requirements.md) — stable requirement IDs,
  commitments, preferences, and unresolved choices.
- [Open design questions](open-questions.md) — decisions that need evidence.
- [Two-character adventure](../games/adventure/README.md) and its
  [selected puzzle rules](../games/adventure/design.md).
- [First conveyor factory slice](factory-slice.md) — selected challenge, construction,
  deterministic transport/production rules, and expected verification traces;
  [playable factory](../games/factory/README.md) includes construction, transport,
  production and diagnosis.
- [Parallel ECS executor](executor.md), [swarm measurements](swarm.md),
  [sparse component retention](sparse-churn.md), and
  [mixed-schedule overhead measurements](executor-overhead.md).
- [Milestone notes](releases/v0.4.0.md) — the v0.4.0 snapshot; later work is
  described by the current guides and merged PRs.

## Contribute

- [Contribution guide](../CONTRIBUTING.md) — choose work, set up, verify, and open a PR.
- [Ask questions](https://github.com/titan-engine/titan/discussions).
- [Find starter issues](https://github.com/titan-engine/titan/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22good%20first%20issue%22)
  or browse the [development board](https://github.com/orgs/titan-engine/projects/1).
- [Quality gates](verification.md) and [evidence lifecycle and failure artifacts](acceptance-evidence.md).
- [Maintainer and agent workflow](workflow.md) and
  [agent runtime skill](../.agents/skills/titan-workflow/SKILL.md).

## Methods and examples

- [Agent iteration procedure](agent-iteration.md) — bounded tasks and honest timing.
- [Inspection failure regression cases](inspection-repair/README.md).
- [Public API boundaries](starter-audit.md) and [historical ECS boundary conclusions](subsystem-audit/README.md).
- [Art comparison](art-iteration/README.md) and [quest journal](journal.md).
