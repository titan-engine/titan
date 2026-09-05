# Save/load boundary

Early save/load and serialization design is an agreed requirement
([R1.47](design-requirements.md)); making every inspectable type serializable is
still undecided ([R2.25](design-requirements.md)). This document establishes the
boundary for that work. Titan does not yet implement a general save-file format,
world serializer, load transaction or persistence API.

Games should define the state needed to resume gameplay. Engine inspection
metadata, render output and a dump of all ECS components are not automatically
the persistent source of truth. Ordinary Rust save-data types can describe that
state without requiring every component, resource or UI element to serialize.

## Persistent gameplay and reconstructed state

For the arena, a future mid-run save would need the player's position, active
enemies and their positions, elapsed game ticks, health, outcome, spawn progress,
current random-generator state, contact cooldown and the complete dash state.
That includes remaining dash ticks, dash cooldown, locked direction and last
movement direction. The initial random seed alone cannot resume an advanced run.
The cooldown values are gameplay state even though the HUD displays them.

For the RPG, the corresponding boundary is the generated-world description or
seed plus changes to that world, player progress, collected items and any other
state that affects subsequent gameplay. The exact save-data types should be
chosen when an actual save/resume scenario is implemented in each game.

| Persist when needed to resume gameplay | Reconstruct or reset on load |
| --- | --- |
| World generation inputs, current RNG state and gameplay changes | Generated render assets and caches |
| Entity relationships using game-owned persistent references | Runtime entity indices/generations and resolved handles |
| Health, inventory, progress, outcomes and simulation timers | HUD text, layout, style, button hover/press/focus |
| Gameplay settings that affect the resumed simulation | Window size, DPI, canvas dimensions and GPU resources |
| Logical references to required assets | Asset handles, surfaces and renderer allocations |

An ECS-based HUD is still derived presentation. Rebuild its entities from game
state and the game's UI definitions, then update their content before extraction.
Do not treat every UI entity as save data merely because it lives in the same
world. Conversely, a UI choice that changes gameplay should update persistent
game state through the normal game action; its visual selection is derived.

Persistent references must be resolved when loading. Runtime `Entity` IDs and
asset handles are identities within a particular running world, not a promised
on-disk identity scheme. This does not require universal textual identifiers for
every object; games can introduce persistent identifiers only where needed.

## Loading and execution boundaries

A future loader should decode and validate bounded input before replacing live
game state. Reconstruct entities, resolve references and rebuild derived assets
and UI at an exclusive simulation safe point. Validate game invariants, required
content and supported format/version before installing the result. These are
requirements for future implementation, not guarantees supplied by the current
nontransactional game-command API.

Clear held keys, buffered taps, pointer gestures, pending injected inputs and
wall-clock accumulation when a load is installed. A button held before loading
must not unexpectedly activate in the restored game. Host pause policy should be
explicit; it is not part of a portable game save.

Loading into the same live session should preserve its protocol request history,
monotonic host frame and state-revision identity. Restore gameplay-relative time
separately and account for the successful load as a state change. A newly started
process may begin its own host clock at zero. Neither case should restore old
inspection credentials, discovery registrations, transport queues or GPU state.

Existing arena recordings describe consumed inputs from restart and verify them
in a fresh game. They are not arbitrary mid-run saves. Loading would begin a new
recording origin or explicitly invalidate restart-based replay; a future format
must identify any required saved starting state. Exact rollback snapshots are a
separate capability and should not dictate the persistent save-file layout.

## First implementation proof and choices still open

Before implementing a general serializer, prove a game-owned mid-run round trip:
save during a dash or contact cooldown, construct a fresh world from the save,
then feed identical subsequent fixed-tick inputs to both worlds. Compare semantic
state and relevant rendered output. Also verify that rebuilt UI reflects restored
values, malformed or unsupported saves do not disturb the current game, stale
input is cleared, and inspection remains correlated after an in-session load.

Storage location, encoding, file-size bounds, schema/version policy, asset-content
identity and persistence of large generated worlds remain to be selected.
Backward compatibility is not currently required. Reject unsupported formats
clearly; do not silently infer a migration or promise durable compatibility.
