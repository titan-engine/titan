# Development status and orientation

Titan is an experimental engine. The v0.4.0 milestone aligns the eight engine
Cargo packages with the source tag; arena/starter packages remain 0.1.0.
Nothing is published to crates.io. Work since that milestone is described in the
current usage guides and linked issues; a package version alone does not identify
all capabilities on `main`.

Start with the [contribution guide](../CONTRIBUTING.md) for setup and choosing
work. The [implementation plan](implementation-plan.md) defines quality gates,
the [vision](vision.md) explains product direction, and the
[design requirements](design-requirements.md) and [open questions](open-questions.md)
distinguish intended capabilities from unresolved choices. Pending work and
approval status live in [Titan Development](https://github.com/orgs/titan-engine/projects/1)
and its issues. Read the relevant issue before implementation; this summary is
not approval for a new feature or release.

## Implemented capabilities

- The [standalone starter](../starters/minimal/README.md) and independent arena
  game demonstrate native, browser and headless authoring without RPG internals.
- Both games use entity-based HUDs. The [RPG quest journal](journal.md) exercises
  column placement, bounded bitmap text and scoped focus.
- Arena supports [save/load](arena-save-load.md), [live-player inspection](live-player.md)
  and [snapshot-backed replay](arena-replay.md), including bounded seeking and
  discrete playback speeds in native and browser players.
- The RPG uses the [shared replay primitives](replay.md) with game-owned snapshots
  that recreate shards and quest state. [RPG playback](rpg-replay.md) is verified
  in native, browser and headless modes; its player does not yet expose arena's
  seek and speed controls.
- The [file-backed sprite exercise](assets.md) loads loose PNGs through the same
  engine `Image` used by procedural art, with startup, resource packaging,
  bounded errors and image-aware replay checks. The [generated image fixture](generated-assets.md)
  demonstrates build-time/lazy generation and disk caching. These bounded
  examples do not constitute a general asset pipeline.
- The [opt-in parallel executor](executor.md) runs compatible typed systems in
  bounded native batches while retaining sequential defaults and a WASM fallback.

Historical milestone briefs, captures and command records retain evidence for
the revisions they describe. In particular, [milestone 2](second-milestone.md)
records the accepted v0.2.0 starter/arena exercise, and the
[v0.4.0 notes](releases/v0.4.0.md) describe that release. Use the current guides
above for behavior added since those snapshots.

## Verification and maintenance

The [maintainer workflow](workflow.md) and `AGENTS.md` describe ownership,
independent reviews and integration. Required PR checks and the merge queue
protect `main`; issue-specific review requirements still apply. Maintainers
approve scope changes and releases. Verify CI for the exact merged main revision
before reporting integration complete.

Preserve RPG reference checksum `f7a298f62ad75c1c`, arena initial
`e096abf94fd12c24` and winning `b5cf61da6f50efd7` unless an intentional visual
change is approved and verified. Keep the README preview at its committed
1280×896 nearest-neighbor resolution; GitHub strips image-rendering CSS, so do
not replace it with the 160×112 source capture.

Run the implementation-plan gates appropriate to each change. Use real
native/browser/headless evidence for runtime changes, maintain CI coverage for
both games and the externally copied starter, and update affected documentation.
