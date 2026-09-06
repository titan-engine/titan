# Titan documentation

Start with what you want to do. Titan is early in development; the
[README](../README.md#what-works-today) describes what is supported today.

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
- [Current implementation overview](handoff.md).
- [Two-character adventure](../games/adventure/README.md) — native/browser control
  foundation and selected puzzle design; [next-milestone proposal](adventure-next-milestone.md)
  awaits maintainer selection, with no further gameplay approved.
- [First conveyor factory slice](factory-slice.md) — selected challenge, construction,
  deterministic transport/production rules, and expected verification traces;
  [playable factory](../games/factory/README.md) includes construction, transport,
  production and diagnosis. See the [next-milestone proposal](factory-next-milestone.md)
  for maintainer selection; further gameplay is not approved.
- [First sound exercise proposal](audio-exercise.md) — pickup cue, playback lifetime
  and device-free verification; audio is not implemented.
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
- [Quality gates](implementation-plan.md) and [evidence lifecycle and failure artifacts](acceptance-evidence.md).
- [Maintainer and agent workflow](workflow.md) and
  [agent runtime skill](../.agents/skills/titan-workflow/SKILL.md).

## Verification and milestone evidence

These reports explain how particular changes were checked. Use the guides above
for current instructions; measurements and environments describe specific runs.

- [Finished factory verification](evidence/factory-verification/README.md) — independent
  player exercises, unfamiliar-author variation and bounded scaling evidence.
- [Shared agent iteration procedure](agent-iteration.md) and
  [initial skeleton measurements](evidence/agent-iteration/README.md).
- [Starter milestone](second-milestone.md), [starter verification](starter-verification.md),
  and [public API boundary audit](starter-audit.md).
- [Arena development exercise](arena-exercise.md), [verification](arena-verification.md),
  and [snapshot verification](arena-save-load.md).
- [Host setup audit](host-setup-audit.md) and [workflow verification](host-workflow-verification.md).
- [ECS-only subsystem boundary audit](subsystem-audit/README.md).
- [Art iteration](art-iteration/README.md) and [quest journal](journal.md).

- [Inspection failure repair evidence](inspection-repair/README.md).
