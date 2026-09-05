# Continue after the RPG quest journal

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

The user approved the RPG quest journal and requested autonomous implementation.
It now exercises shared column layout, bounded bitmap text, scoped keyboard focus
and modal input isolation. Gameplay replay uses a canonical closed-journal image;
live captures show the actual panel. See [journal behavior and verification](journal.md)
and the [implementation plan](implementation-plan.md). Keep the journal scope
bounded; assets, difficulty settings, replay scrubbing and speed controls are not
selected work.

Working preferences: use subagents for substantial independent work, keep the
main context compact, make frequent coherent commits, and continue autonomously
within the agreed scope until real user input is needed. Push when authorized
and verify CI against the exact pushed revision. Do not publish crates or create
release tags without authorization.

Preserve RPG reference checksum `f7a298f62ad75c1c`, arena initial
`e096abf94fd12c24` and winning `b5cf61da6f50efd7` unless an intentional visual
change is approved and verified. Keep the README preview at its committed
1280×896 nearest-neighbor resolution; GitHub strips image-rendering CSS, so do
not replace it with the 160×112 source capture.

Run the implementation-plan gates appropriate to each change. Use real
native/browser/headless evidence for runtime changes, maintain CI coverage for
both games and the externally copied starter, and update current docs/evidence.
