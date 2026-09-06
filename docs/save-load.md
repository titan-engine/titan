# Save/load boundary

The RPG's [file-backed player image](assets.md) is retained from startup across
snapshot loading and restart. Snapshots do not serialize its bytes; a fresh host
loads its configured image before restoring game state. Replay additionally
checks final pixels, so it requires matching art.

Early save/load and serialization design is an agreed requirement
([R1.47](design-requirements.md)); making every inspectable type serializable is
still undecided ([R2.25](design-requirements.md)). This document establishes the
boundary for that work. [Arena snapshots](arena-save-load.md) and the
[RPG v1 snapshot](rpg-replay.md#snapshot-behavior) exercise it with separate,
game-owned formats and validated load operations. The RPG format is implemented
in [`fixtures/rpg/src/snapshot.rs`](../fixtures/rpg/src/snapshot.rs). Titan does
not implement a general save-file format, world serializer or engine-wide
persistence API.

Games should define the state needed to resume gameplay. Engine inspection
metadata, render output and a dump of all ECS components are not automatically
the persistent source of truth. Ordinary Rust save-data types can describe that
state without requiring every component, resource or UI element to serialize.

## Persistent gameplay and reconstructed state

The arena snapshot contains the player's position, active
enemies and their positions, elapsed game ticks, health, outcome, spawn progress,
current random-generator state, contact cooldown and the complete dash state.
That includes remaining dash ticks, dash cooldown, locked direction and last
movement direction. The initial random seed alone cannot resume an advanced run.
The cooldown values are gameplay state even though the HUD displays them.

The RPG's bounded JSON v1 snapshot stores its format version and game seed;
player and shrine positions; the names and positions of remaining shards; the
collected-shard count; and shrine activation. It rejects unsupported identity,
out-of-map coordinates, inconsistent quest state, empty or overlong shard names,
more than 256 remaining shards and encoded input over 64 KiB. Its game-owned
Rust types deliberately do not form a general engine persistence API.

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

## Shipped loading and execution boundaries

The RPG and arena loaders decode and validate bounded input before replacing live
game state. They validate the initialized target and game invariants before
installation, reconstruct game-owned state, and rebuild derived presentation.
The RPG recreates remaining shard entities, updates player and shrine positions,
restores shrine activation and quest progress, rebuilds quest text, and resets
its transient journal. Its loaded world retains startup image assets rather than
serializing their bytes. These guarantees belong to the documented game formats,
not to the current nontransactional command API or a general Titan loader.

The RPG clears scheduled and current input plus movement-repeat state when a load
is installed; the arena similarly clears stale live input. A button held before
loading therefore does not unexpectedly activate in the restored game. Host
pause policy is explicit and is not part of either portable game save.

Loading into the same live session should preserve its protocol request history,
monotonic host frame and state-revision identity. Restore gameplay-relative time
separately and account for the successful load as a state change. A newly started
process may begin its own host clock at zero. Neither case should restore old
inspection credentials, discovery registrations, transport queues or GPU state.

Arena v2 recordings embed their initial game-owned save and begin a new segment
after a successful load or restart. They verify complete final state and pixels
in a fresh game and support [interactive playback](arena-replay.md). Historical
v1 recordings retain their restart origin. Exact rollback snapshots are a
separate capability and should not dictate the persistent save-file layout.

## Implemented proofs and choices still open

The arena proves a game-owned mid-run round trip during dash/contact state. The
RPG proof saves after collecting one shard, finishes the quest, restores that
snapshot, and repeats the remaining route to the same complete state and exact
software pixels. Both verify semantic state and derived presentation; malformed
or unsupported saves leave the live game unchanged. See [arena save/load](arena-save-load.md)
and [RPG snapshot and replay acceptance](rpg-replay.md#acceptance) for commands
and format-specific guarantees.

Beyond the games' bounded JSON formats, general storage location,
encoding, schema/version policy, asset-content identity and persistence of large
generated worlds remain to be selected.
Backward compatibility is not currently required. Reject unsupported formats
clearly; do not silently infer a migration or promise durable compatibility.
