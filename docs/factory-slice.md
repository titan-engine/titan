# First conveyor factory slice

This is the selected game design for [issue #88](https://github.com/titan-engine/titan/issues/88),
implemented by the [standalone factory](../games/factory/README.md). It specifies
the game contract and independent verification expectations. The
[documentation index](README.md),
[standalone starter](../starters/minimal/README.md), [vision](vision.md), and
[requirements](design-requirements.md) supply the engine context. These rules
belong to the game; they do not select a general engine transport framework.

## Challenge and world

Build a route that extracts **ore**, turns each ore into one **plate**, and delivers
**10 plates**. There is no time limit, loss state, construction cost, or finite
building stock. The player can experiment and repair a stalled route. No workers,
power, research, economy, dedicated splitters/mergers, underground belts, manual
item carrying, or additional recipes are selected.

The world is a fixed 12 by 8 square grid. Coordinates are integers: x=0..11 from
left to right, y=0..7 from top to bottom. North is (0,-1), east (1,0), south (0,1),
west (-1,0). No wrapping, diagonals, height, obstacles, or random generation.
Every tile is buildable ground except for the placement constraints below.
One inexhaustible ore deposit is terrain at (1,3). One delivery building starts
at (10,3), facing east (its input faces west); it is fixed, cannot be removed or
rotated, and is the only delivery building. All other tiles start empty.

The initial run has tick 0, delivered=0, extracted=0, discarded_ore=0,
discarded_plate=0, no items or machine progress, and outcome Running. The player
starts with the conveyor tool facing east; the camera frames the entire grid.
The visible objective names the deposit, processor recipe, and delivery target.

A reference solution, built before advancing tick 1, is:

| Tile(s) | Structure | Facing |
| --- | --- | --- |
| (1,3) | Extractor | East |
| (2,3), (3,3), (4,3) | Conveyor | East |
| (5,3) | Processor | East |
| (6,3), (7,3), (8,3), (9,3) | Conveyor | East |
| (10,3) | Initial delivery | East |

This is an example, not a required layout. Detours and multiple processors are
valid. Only one extractor can exist because there is one deposit tile.

## Occupancy, construction, and ports

Every structure occupies exactly one tile, including machines. A tile contains
at most one structure; item slots are owned by that structure, not independent
ground occupancy. Extractors require the deposit. Conveyors and processors may
occupy any empty tile except the deposit. Delivery is never player-placeable.
Placement cannot replace an occupied tile. Invalid coordinates, kinds, facing
values, occupied tiles, or terrain reject the entire operation without mutation.

Facing is one of N/E/S/W. Place supplies a kind, tile and facing. Rotate turns an
existing conveyor, extractor, or processor clockwise by 90 degrees, preserving
its items and progress. Remove deletes one such structure and accounts for its
contents as described below. Rotating/removing an empty tile or the delivery
rejects without mutation. Facing out of the world or toward an incompatible
neighbor is legal construction: show the disconnected port; do not reject it.

An output points at the adjacent tile in the facing direction. An extractor has
only this output. A processor has this output and one input on its rear face
(opposite its facing). Delivery has only its fixed west input. A conveyor has
one output and inputs on its other three faces. Thus ordinary conveyors can
turn corners and accept competing side/rear feeds, but cannot split an output.
Head-on conveyors cannot connect through their output faces.

A directed connection exists only when the source output touches an accepting
input face of an adjacent structure. Any structure with an output may connect
directly to any structure with a compatible input: an intervening conveyor is
not mandatory for a connection. Conveyors accept ore or plates, processors only
ore, and delivery only plates. Incompatible items stay at their source even if
the geometry connects. No automatic routing, pulling, teleporting, or swapping.
Connections are derived again after edits; no persistent connection IDs affect
simulation order.

Player and tool construction use the same validated game operations. Record an
ordered sequence of boundary operations and advances, for example
`place(extractor,1,3,E); place(conveyor,2,3,E); advance(60)`.
This notation specifies semantics, not a CLI spelling or serialization format.
Operations execute serially at safe boundaries between ticks, in recorded order;
a later operation sees earlier edits. There is no partially applied placement or
concurrent edit during a tick. Rejected operations record their rejection and
change no simulation state. The construction implementation defines its concrete
sequence encoding and actionable error codes under
[issue #89](https://github.com/titan-engine/titan/issues/89).

## State, capacities, and time

Use integer fixed simulation ticks at 60 Hz, independent of render frames and
wall time. The first advance from initial state executes tick 1. No floating
point distance, animation, entity allocation order, or hash traversal determines
an item transfer. Rendering can interpolate motion but cannot change occupancy.

| Structure | Bounded state | Production |
| --- | --- | --- |
| Extractor | One ore output slot; progress 0..59 | One ore per 60 eligible ticks |
| Conveyor | One slot holding ore or plate | At most one outgoing item per tick |
| Processor | One ore input, one in-process ore, one plate output; remaining 0..120 | One ore becomes one plate in 120 work ticks |
| Delivery | Delivered plate count 0..10; no item buffer/output | Consumes accepted plate during transfer |

The processor's in-process ore is distinct from its input queue. Remaining=0
with an in-process ore means finished work waiting for output room; remaining=0
without it means idle. There is no partial ore creation or fractional item.
The extracted counter increments only when an extractor creates an actual ore.
Use counters that cannot wrap silently; exceeding a representation limit must
stop with a diagnostic, never alter the accounting. All running behavior below
assumes representable state.

## Exact tick order

Each Running tick consists of these phases, in order:

1. **Transfer snapshot.** Snapshot all slots and connections after boundary edits,
   before any production. Each nonempty output (including a conveyor slot) proposes
   at most one transfer to its facing neighbor. It is eligible only for a matching
   port, accepted item type, and space in the destination's snapshot input/slot.
   A full slot stays unavailable for this tick even if its item will leave.
   Delivery has space while its snapshot count is less than 10.
2. **Resolve and commit transfers.** Sort proposals by source tile `(y,x)` ascending
   (top to bottom, then left to right). Reserve destination capacity in that order;
   reject later contenders once reserved. Commit accepted transfers simultaneously,
   removing exactly one source item and adding exactly one destination item, or
   incrementing delivered for delivery. Received items cannot move again this tick.
   Each structure has one output and one receiving slot, so no further tie-breaker
   is needed. Priority is fixed, not round-robin: starvation under contention is
   permitted and inspectable.
3. **Completion.** If delivered reaches 10, set outcome Complete and completion_tick
   to this tick. Skip production in this tick. Freeze subsequent simulation ticks
   and construction; inspection and restart remain available. A host/protocol frame
   clock may continue, but the game's tick and completion_tick stay fixed.
4. **Production.** For each extractor, if its output is full, preserve progress
   without incrementing. Otherwise increment progress by one; when it reaches 60,
   create one ore in the output, increment extracted, and reset progress to zero.
   For each processor, first handle the batch present at the start of this phase:
   decrement positive remaining by one. If remaining is now zero and the output
   is empty, replace the in-process ore with one plate in the output. If output
   is full, keep the finished batch at remaining=0. Then, if no batch remains and
   the input has ore, move that ore into the in-process slot with remaining=120.
   A newly started batch receives no work tick until the next tick. Starting is
   allowed while output is full; at most one finished batch can wait behind it.

Production reads the post-transfer state. Thus an extractor can begin refilling
on the tick its output leaves; a processor can start ore received that tick;
a finished processor batch can enter output vacated that tick. Production cannot
transfer its new output until the next tick. Machines do not share mutable
production state, so their traversal order has no effect on results.

This conservative snapshot policy intentionally leaves a one-tick gap behind a
moving item. A packed belt does not shift as one chain in the same tick. It avoids
recursive capacity propagation and makes cycles as well-defined as straight
routes. Fixed positional priority is simple to reproduce but offers no fairness
guarantee. These are selected first-slice tradeoffs, not optimization targets.

## Backpressure, edits, and accounting

A missing/out-of-bounds neighbor, mismatched input face, rejected item type, full
destination, or losing contention keeps the item unchanged. Report those reasons
in that precedence order; distinguish empty source from a stalled item. Machine
state distinguishes extracting, output blocked, waiting for ore, processing,
finished batch blocked, and complete. Inspection must expose tile/facing, slot
item types, progress/remaining, counters and outcome. Visual arrows and item
positions should agree with these fields; UI design belongs to
[issue #92](https://github.com/titan-engine/titan/issues/92).

Disconnected networks retain all items and stall once buffers fill. No timeout
removes items. Cycles are legal; a partially occupied cycle moves only into
snapshot-empty tiles. A completely full cycle remains jammed indefinitely until
an edit frees space. No special loop breaker or global path search runs.

Removal is an explicit discard operation, with its affected item counts visible
to the player/tool. Add each contained ore (including an in-process ore, even at
remaining=0) to discarded_ore and each output/conveyor plate to discarded_plate.
Then clear slots and progress and remove the structure. Extractor partial
progress represents no item and adds nothing. There are no loose drops, refunds,
recovery inventory, or automatic delivery on removal. Replacing the structure
starts empty with zero progress. Rotation never discards or reclassifies items;
new connections apply to the next tick. Removal followed by placement at the
same boundary still discards the old contents exactly once.

At every boundary in one run, the invariant is:

```text
extracted = ore in all slots + in-process ore + plates in all slots
          + delivered + discarded_ore + discarded_plate
```

Each plate represents one extracted ore. For transport-only test fixtures, add
the explicitly seeded item count to the left side. Seeding is a test setup, not
a player action. No production completion changes the total. Rejected edits,
rotation, blocked transfers, and completion freeze preserve it. Explicit removal
is the only within-run discard mechanism.

Restart is available both Running and Complete. It reconstructs the initial
world above, removes every player structure and item, zeros all counters and
progress, clears completion_tick, and sets tick=0 and outcome Running. It resets
construction selection/facing and camera to their initial values. No previous
held input or pending construction leaks into the new run. In a recorded sequence,
operations explicitly following restart address the new run. Restart begins a
new accounting epoch; it does not add the old contents to new discard counters.
Saving, loading, and interactive replay UI are not selected here; the state and
ordered operations are explicit to support deterministic tests and later work.

## Expected traces for independent verification

All rows describe state **after** the named tick. `-` is an empty slot. Labels
such as A/B identify items only for explaining traces; unique runtime item IDs
are not required. These fixtures specify expected behavior;
[finished verification](evidence/factory-verification/README.md) records runtime
evidence and its limitations.

### Snapshot capacity and one-hop movement

Three east conveyors at (2,2), (3,2), (4,2), with no other structures nearby:

| Tick | (2,2) | (3,2) | (4,2) | Explanation |
| --- | --- | --- | --- | --- |
| 0 | A | B | - | Two seeded ore |
| 1 | A | - | B | A sees a full middle tile; B advances |
| 2 | - | A | B | A advances; B has no neighbor |
| 3 | - | A | B | Middle full-destination stall; last disconnected |

For an initially empty middle and last tile with only A seeded, tick 1 puts A
in the middle and tick 2 puts A in the last. It never crosses two tiles per tick.

### Corners, competition, and cycles

An east conveyor at (3,3) accepts a south-facing source at (3,2) and an
east-facing source at (2,3). With both sources holding ore A/B and the destination
empty, tick 1 accepts A from (3,2), because `(2,3) < (3,2)` in `(y,x)` order;
B remains with reason contention. If (4,3) is an empty east conveyor, tick 2 moves
A there but B stays (destination was full). Tick 3 lets B enter (3,3).

For a four-tile clockwise loop, use (2,2) E, (3,2) S, (3,3) W, (2,3) N.
One seeded item at (2,2) visits (3,2), (3,3), (2,3), (2,2) after ticks 1..4.
Four seeded items never move. Remove one occupied belt: three items remain and
one discard is recorded; place an empty belt with the same facing before the
next tick. Exactly the predecessor of that empty tile transfers next tick.

### Production, blocked output, and removal

An isolated empty extractor has progress=59 after tick 59, creates its first ore
at tick 60 with progress=0, and stays unchanged through tick 180. If its ore
leaves on tick 181, progress becomes 1 that tick; the next ore appears at tick 240.

An idle processor with one seeded input ore starts a batch at tick 1 with
remaining=120. At tick 120 remaining=1; tick 121 creates one output plate. If a
plate was already in output and cannot leave, the new batch instead waits at
remaining=0 through tick 121 and beyond. When the old plate leaves at tick 122,
the finished batch becomes an output plate in that same tick; it cannot transfer
until tick 123. An input ore waiting behind it starts at tick 122 with remaining=120.
Removing a processor with input ore, a finished blocked batch, and an output
plate records discarded_ore+=2 and discarded_plate+=1, with no delivery.

### Complete reference route

For the reference solution built at tick 0, no further edits:

| Tick | Expected event |
| --- | --- |
| 60 | First ore appears in extractor output |
| 61, 62, 63 | First ore reaches conveyors (2,3), (3,3), (4,3) |
| 64 | First ore enters processor; batch starts with remaining=120 |
| 120 | Second ore appears in extractor output |
| 124 | Second ore enters processor input and waits |
| 184 | First plate appears; second batch starts with remaining=120 |
| 185, 186, 187, 188 | First plate reaches (6,3), (7,3), (8,3), (9,3) |
| 189 | First plate delivered; delivered=1 |
| 309 | Second plate delivered; delivered=2 |
| 1269 | Tenth plate delivered; outcome Complete; production skipped |

Plate k is delivered at tick `189 + 120*(k-1)` for k=1..10. After completion,
advance requests do not change game state; construction rejects as complete.
Restart restores the empty challenge and another identical construction/advance
sequence reproduces the trace. A rejected attempt to remove delivery must leave
all counters and its west input unchanged.

Verification must assert the above traces and conservation at
every tick, including invalid edits, rotation of occupied structures, wrong-type
inputs, out-of-bounds outputs, and reset from blocked and completed states.
Run the same fixtures natively and in actual WASM for transport
and production changes. Follow [quality gates](verification.md) and the
[runtime workflow](../.agents/skills/titan-workflow/SKILL.md) for runtime evidence.
This design does not change existing RPG/arena checksums or platform claims.
