# Continue after the file-backed RPG sprite

Work in the Titan repository on `main`. The v0.4.0 milestone aligns the eight
engine Cargo packages with the source tag; arena/starter packages remain 0.1.0.
Nothing is published to crates.io. Check actual Git/CI state before new work.

Read `docs/implementation-plan.md`, `docs/vision.md`,
`docs/design-requirements.md`, `docs/open-questions.md` and the repository's
`.agents/skills/titan-workflow/SKILL.md`. The design-requirements document captures
both original planning answer rounds, including entity-based UI. Keep these
sources consistent when completing work; do not infer missing requirements from
the demos alone.

Completed: arena/RPG entity-based HUD, arena save/load, shared replay engine
primitives adopted by both games, game-owned RPG snapshots that recreate shards,
and native/browser/headless playback verification. Read `docs/replay.md`,
`docs/rpg-replay.md` and `docs/arena-replay.md` for the exact boundary and evidence.
Both pushed implementation revisions passed CI before the release bump.

The quest journal is complete; see [journal behavior](journal.md). The subsequent
approved exercise loads the existing player sprite from a loose PNG through the
same engine `Image` used by procedural art. Native/headless/browser startup,
resource packaging, bounded errors and image-aware replay are implemented. See
[asset behavior and evidence](assets.md) and the [implementation plan](implementation-plan.md).
Hot reload, caching, other formats, difficulty settings and replay scrubbing are
not selected work. Review the remaining plan before proposing another exercise.
No next feature or new release tag is approved.

Working preferences: use subagents for substantial independent work, keep the
main context compact, make frequent coherent commits, and continue autonomously
within the agreed scope until real user input is needed. Finish review and local
verification before one batch push, then verify CI
against the exact pushed revision. Prefer more useful engine exercises before
another release; frequent local commits do not require individual pushes. Do not
publish crates or create release tags without authorization.

Preserve RPG reference checksum `f7a298f62ad75c1c`, arena initial
`e096abf94fd12c24` and winning `b5cf61da6f50efd7` unless an intentional visual
change is approved and verified. Keep the README preview at its committed
1280×896 nearest-neighbor resolution; GitHub strips image-rendering CSS, so do
not replace it with the 160×112 source capture.

Run the implementation-plan gates appropriate to each change. Use real
native/browser/headless evidence for runtime changes, maintain CI coverage for
both games and the externally copied starter, and update current docs/evidence.
