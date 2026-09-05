# First-slice design

Status: selected rules for [issue #80](https://github.com/titan-engine/titan/issues/80),
subject to maintainer PR review. This document specifies intended behavior;
no adventure runtime or playtest result is claimed. The bounded outcome is one
player, two freely switchable characters and two cooperative rooms. Names
**Jumper** and **Strong** are functional labels, not a narrative commitment.

## Controls and presentation

| Action | Native and focused browser player | Rule |
| --- | --- | --- |
| Move | WASD or arrow keys | Camera-relative planar movement; W/up is north (-Z), D/right is +X. |
| Jump | Space | Press edge, grounded only; no auto-repeat, double jump or buffered landing jump. |
| Switch | Q | One press immediately selects the other character, even in midair or during a block move. |
| Push | E plus a cardinal direction | Strong only; one adjacent rail step per E press. |
| Restart room | R | Restore the current room immediately, including from completion UI. |
| Pause/resume | P or visible host button | Freeze game ticks; no gameplay input accumulates while paused. |

The browser consumes these keys only while the player has focus, preventing
Space scrolling there. Space is jump, including in the browser; do not inherit
the collection-room browser's Space-to-pause mapping. Losing focus pauses and
clears held keys and taps. Resume requires an explicit action. Native Escape
closes the player; browser Escape releases focus and pauses.

Use a fixed elevated perspective camera per room: eye `(6, 14, 17)` metres,
look-at `(6, 0, 4)`, +Y up, vertical field of view 50 degrees, near/far 0.1/50 m.
There is no rotation, character tracking, switch pan or zoom. Render a 16:9
viewport with letterboxing, preserving composition at 960x540 and 1280x720;
smaller surfaces show a size hint. These are presentation acceptance targets,
not an existing platform guarantee. Omit foreground exterior wall meshes; tall
partition walls may use a cutaway visual while retaining full collision. Keep
both characters, plate symbols and exit visible; validate occlusion in actual
native/browser captures during implementation.

Jumper has a narrow silhouette and triangle marker; Strong has a broad silhouette
and square marker. An active ring and text name identify control. Plate-to-door
links have matching symbols, and pressed/open/obstructed states use shape or
text as well as color. Show controls, current objective, both exit occupancy
indicators, and brief rejection feedback (for example, "Block occupied").
The start screen explains switching, jumping and getting both characters to the
exit; starting enters room 1 with Jumper selected.

## Movement, switching and simulation

All dimensions below are selected tuning values in integer millimetres.
Simulation is fixed at 60 ticks/s; rendering interpolates without changing rules.
Positions describe the character's foot center. Both characters have an axis
aligned body, half-width 200 in X/Z and height 900. They share horizontal speed
60 per tick (3.6 m/s); diagonals use 42 on each axis. Opposing directions cancel.
Use swept collision against static solids and blocks, resolving X then Z, with
no penetration or step-up. A character must jump to mount any raised surface.
Characters do not collide with, push, support or carry each other: they may pass
through and share floor/plate/exit space. This prevents corridor body-blocking
and character stacking from bypassing ability gates.

A grounded jump sets vertical velocity to 180 for Jumper or 100 for Strong.
Each airborne tick subtracts gravity 10, then moves vertically by the resulting
velocity, sweeping for ceiling or support contact. From level ground the apex
is respectively 1530 or 450 above the launch surface (17 or 9 rising ticks;
one zero-velocity tick follows). Air control uses the same horizontal speed.
Ceiling contact cancels upward velocity; descending contact lands on the highest
crossed support. Any positive footprint overlap with a horizontal support can
support a character; edge-only contact cannot. No coyote time or jump buffering.
Walking off a support begins gravity that tick. The surrounding walls are solid;
only the marked pit in room 2 allows a fall below floor level.

Switch changes only the control target, never positions, velocities or puzzle
state. The old character stops receiving horizontal input on that tick. Grounded
inactive characters stay put and continue to press plates; airborne inactive
characters lose horizontal motion but continue gravity and landing. The newly
selected character receives **no movement, jump or push on the switch tick**.
Every currently held movement/jump/push action is suppressed until that action
is released and pressed again; Q also needs release before another switch.
Apply this at logical-action level after combining physical aliases, and to
injected input as well as keyboards. A fresh direction on a later tick works
without waiting for unrelated suppressed actions. Switching during a jump is
safe but can make the old character miss its landing; recovery rules still apply.

Per tick, use this precedence: restart; switch/input filtering; push request;
horizontal then vertical character movement; fall recovery; plate sampling;
door state; completion. Restart consumes the tick. Process characters in stable
Jumper-then-Strong order. Solid collision uses the door state from the previous
tick, so a newly pressed plate opens passage for movement on the next tick.
Paused game time does not advance. Clear and release-gate gameplay input on
resume, room transition and restart, using the same held-action policy as switch.

## Plates, door and exit

Each room has one door and two floor plates. Its door is requested open while
**either** plate has a grounded character whose foot center is inside the plate's
600x600 square (boundary included) on that plate's support height. Blocks do not
press plates. One plate is on the starting-side ledge; the other is on the far
side. Plates are hold-to-open, with no timer or permanent latch. The far plate
allows the first character through to hold the door for the partner.

When neither plate is pressed, the door closes unless a character body overlaps
the doorway volume. If obstructed, it remains fully open and reports
`open_obstructed`; it never crushes, shoves or traps a body. Close on the first
unobstructed tick with no plate active. Grounded or airborne bodies count.
The block rail never intersects a doorway. A closed door is a full-height solid,
so jumping cannot bypass it. A just-opened door cannot close on a body already
inside; collision edge contact alone is not obstruction.

Completion requires **both** characters grounded with their complete X/Z
footprints inside the exit rectangle at the end of the same tick. One character
arriving changes only its exit indicator. Neither a jump above the exit nor one
character leaving before the other arrives completes it. Latch completion and
freeze puzzle simulation once both qualify. Room 1 shows "Room complete" with
an explicit Continue button/Enter press; there is no timed automatic transition.
Continue constructs room 2 from its initial state, selecting Jumper. Room 2
shows "Slice complete" with Restart room and Play again; Play again constructs
room 1. R always restarts the displayed room; it never secretly returns to room 1.

## Heavy block

Only room 2 contains a block: a 900x900 footprint, height 750, ground-supported.
It occupies one of three socket centers on a north/south rail:
`(5500,5500)`, `(5500,4500)`, `(5500,3500)`. It starts at the first socket.
North and south pushes are allowed when the required stance is reachable. The
final north socket is terminal: its reverse stance is inside the ledge, so R is
the way to restore the block after that placement. There is no pulling, grabbing,
throwing, stacking, friction simulation or rotation.

For E plus exactly one effective cardinal direction, Strong must be grounded on
the floor, within 100 of the stance point one metre behind the block relative
to that direction, with no jump on that tick. A valid request moves the block
atomically one socket in the requested direction; Strong stays at the stance
point and receives no movement/jump that tick. A short cosmetic slide may depict
the move, but simulation support/collision use the destination immediately.
Switching never cancels or partly applies an accepted move.

Reject requests by Jumper, from the air, with zero/multiple directions, from an
invalid stance, off the rail, or beyond its endpoints. Reject if either character
is supported by the block, or any character/solid overlaps the swept block volume
including the destination; ignore the block itself and its supporting floor.
Strong's valid stance is outside that volume. Exact face contact is permitted;
positive overlap is blocked. This includes airborne characters below the top;
a body entirely above the swept volume is clear unless supported on the block.
On rejection, block and characters remain unchanged for the push operation;
ordinary character movement can still proceed that tick. Report a stable reason:
`wrong_character`, `not_grounded`, `invalid_direction`, `invalid_stance`,
`rail_end`, `block_occupied`, or `path_obstructed`, in that priority order.
A jump plus push request is rejected as `not_grounded` (jump requested), then
ordinary jumping proceeds. Holding E never chains pushes; release it each time.

## Room geometry

Both rooms use the same 12x8 m footprint. Coordinates are metres in the tables
and diagrams: X increases east/right, Z increases south/down, Y is height.
Rectangles give occupied extents, not tile centers. All unlisted interior floor
is solid at Y=0. Exterior bounds at X=0/12 and Z=0/8 are 4 m tall collision walls.
Static volumes and the door extend upward from floor to their specified height;
platform tops support characters. There are no invisible routes behind walls.

| Shared object | X extent | Z extent | Height / meaning |
| --- | --- | --- | --- |
| North partition | [7,8] | [0,4] | Solid to Y=4 |
| South partition | [7,8] | [6,8] | Solid to Y=4 |
| Door D | [7,8] | [4,6] | Closed solid to Y=4; entirely absent when open |
| Far-side plate B | [9.7,10.3] | [4.7,5.3] | Floor, holds D open |
| Exit E | [10,12] | [1,3] | Floor; both bodies must fit |
| Jumper start J | 1.5 | 6.5 | Foot Y=0 |
| Strong start S | 3.5 | 6.5 | Foot Y=0 |

The partition is too tall to jump even from the block or ledges: the highest
room-2 ledge apex is 3.53 m, below its 4 m top. The closed door shares that height.

### Room 1: hold the way open

| Object | X extent | Z extent | Height / meaning |
| --- | --- | --- | --- |
| Teaching ledge L | [1,3] | [1,3] | Solid to Y=1 |
| Starting-side plate A | [1.7,2.3] | [1.7,2.3] | On ledge at Y=1, holds D open |

Plan sketch (one character per square metre; A/B mark plate cells; tables are
authoritative where a plate lies across cell boundaries):

```text
    X 012345678901
Z 0   .......#....
  1   .LL....#..EE
  2   .LA....#..EE
  3   .......#....
  4   .......D....
  5   .......D..B.
  6   .J.S...#....
  7   .......#....
```

Solution route:

1. Move Jumper to `(2,3.5)` and jump north onto the 1 m ledge. Stop on A at
   `(2,2)`. Its height exceeds Strong's 0.45 m jump; Jumper's 1.53 m clears it.
2. Switch to Strong, release held actions, walk through the doorway along Z=5
   and stop on B at `(10,5)`. A remains pressed by inactive Jumper.
3. Switch to Jumper, walk off the ledge to the south and land. A releases but B
   holds D open. Cross along Z=5, then walk north into E, for example `(10.5,2)`.
4. Switch to Strong and leave B for E, for example `(11.5,2)`. The door closes
   behind both; room completion latches. Continue starts room 2.

A single character cannot substitute for both: Strong cannot reach A, and Jumper
leaving A closes the distant door before it can cross. Releasing B too early
may close D while Jumper is still west; return Strong to B. Standing in D holds
it open harmlessly. Missed ledge jumps land on the floor and can be retried;
there is no pit or irreversible arrangement in this teaching room.

### Room 2: build a step, then exchange places

| Object | X extent | Z extent | Height / meaning |
| --- | --- | --- | --- |
| High ledge L | [4,7] | [1,3] | Solid to Y=2 |
| Starting-side plate A | [5.2,5.8] | [1.7,2.3] | On ledge at Y=2, holds D open |
| Pit O | [1,3] | [3,5] | No floor; no bottom support |
| Block K | Center (5.5,5.5), then (5.5,4.5) or (5.5,3.5) | Rail along -Z/+Z | Foot Y=0; top Y=0.75 |

```text
    X 012345678901
Z 0   .......#....
  1   ....LLL#..EE
  2   ....LAL#..EE
  3   .OO..r.#....
  4   .OO..r.D....
  5   .....K.D..B.
  6   .J.S...#....
  7   .......#....
```

`r` marks the other block sockets, not raised geometry. The pit teaches recovery
without lying on the required route. Keep its rim visually distinct; it has no
collectible or secret route.

Solution route:

1. Park Jumper south/west of the rail. Select Strong at `(5.5,6.5)`, face north,
   press E to move K from Z=5.5 to 4.5. Release E, walk to `(5.5,5.5)`, then push
   north again to Z=3.5. The rail ends here, 0.05 m short of the ledge face.
2. Switch to Jumper. Approach from the south around the pit, jump from about
   `(5.5,4.5)` north onto K, release Space and settle on its 0.75 m top. Release
   movement before overshooting the small top. Strong must stand clear.
3. Jump north from K onto L and stop on A at `(5.5,2)`. The remaining rise is
   1.25 m, within Jumper's 1.53 m jump. Floor-to-ledge is 2 m and impossible;
   Strong cannot mount the 0.75 m block or make the remaining rise.
4. Strong walks east/south around K to cross D at Z=5 and holds B. Jumper leaves
   A, drops south off L (steer east of K), crosses the doorway and enters E.
   Strong follows into E. Slice completion latches.

An edge jump may reach L from the intermediate socket. **Do not use distance
alone as the socket gate:** require K at the final socket before A can activate
in room 2. Show "Move the block to the marked end" if Jumper reaches A otherwise.
The final socket has a visible outlined target linked to A. This explicit plate
condition makes the intended two pushes part of the puzzle; it is an authored
rule, not a physics assumption.

A failed jump lands on floor/block or enters the pit. A character on K prevents
pushing; switch and step off before retrying. A character in the push corridor
causes rejection rather than displacement. At the intermediate socket, Strong can reach the south-push stance `(5.5,3.5)`
via the east side (X about 6.5) and reverse the first push. At the final socket,
the reverse stance `(5.5,2.5)` is inside L and unreachable on the floor. This
terminal placement is the intended solution, not a failed arrangement: it leaves
the jump and doorway routes clear. R restores the starting arrangement if the
player wants to retry. B still provides the return path if D closes early.

## Recovery and reset

If either foot drops below Y=-2 m, reset **both** characters and all puzzle state
to the current room's initial state on that tick. This deliberately uses a whole
room retry instead of a potentially occupied last-safe-position teleport. Clear
velocities, held-input eligibility, plate/door states, block position, completion,
and select Jumper. Display "Fell — room reset" briefly after recovery. Do not
reset the room merely for descending from a ledge onto valid floor.

R uses exactly the same reconstruction without the fall message. Reset the room
simulation counter but retain monotonic host frame/provenance and increment the
session/reset generation, following the collection-room inspection convention.
Discard pending input from the old room and invalidate its pending captures.
There are no lives, persistent saves, checkpoints, retained solved plates or
block arrangements between rooms. Starting, Continue and Play again reconstruct
the destination; there is no backward room navigation in this slice.

## Verification contract for implementation

These are required scenarios for subsequent implementation, not tests executed
by this design issue. Inspect active character, both positions/velocities/support,
room/tick/reset identity, block socket, plate conditions, door state, exit
occupancy and last rejection. Record fixed input and bounded ticks for each
route; replay should reproduce semantic state in headless native and actual WASM.

| Scenario | Expected result |
| --- | --- |
| Complete each route and the sequence | Both distinct characters qualify; room transition restores initial state; final completion freezes. |
| Try Strong at each height gate, Jumper without K, and A with K off target | Height/plate restrictions prevent bypass; rejection feedback explains the target rule. |
| Switch with movement/Space/E held, including midair and alias keys | Immediate selection; no inherited action; release/repress restores control; gravity continues. |
| Switch while inactive character stands on A/B | Plate remains active until that character actually leaves support/plate. |
| Press/release plate; enter doorway and release last plate | Defined one-tick collision timing; obstruction holds open without crushing; closes after clearance. |
| Reverse the intermediate push; try reversing final placement; restart | Intermediate reverse succeeds from its floor stance; final reverse has no valid floor stance; R restores K to its initial socket. |
| Push each direction at rail ends; push as Jumper, airborne or with ambiguous direction | Stable rejection with no partial block motion. |
| Push occupied K, push into grounded/airborne body, stand just outside swept volume | Reject overlaps/support occupancy; accept clear moves; never carry or trap characters. |
| Miss jumps, drop from ledge, fall into pit while either character is active | Ordinary landing preserves state; pit threshold reconstructs whole room even for inactive fall. |
| Only one at E; both above E; one leaves before other arrives | No completion until both are grounded and fully inside together. |
| R during jump/push/completion; transition with held keys; focus loss/resume | Defined reconstruction/input gating, no leaked action, room identity and capture provenance updated. |
| Native/browser captures at both sizes | Fixed view, readable symbols/prompts, both characters visible, door/cutaway geometry understandable. |

## Selected scope and alternatives

The selected rules are continuous planar movement, fixed-step jumps, atomic
rail pushes, noncolliding characters, two hold plates per door, full-room fall
recovery, and an explicit final-socket condition in room 2. Free block physics,
carrying partners, plate timers/latches, rotating cameras, gamepads, additional
abilities/rooms, checkpoints and persistent saves are not selected. Future
milestone decisions belong in [issue #87](https://github.com/titan-engine/titan/issues/87).
This design does not settle general engine physics, scene authoring or framework
extension policy; [#17](https://github.com/titan-engine/titan/issues/17) and
[#18](https://github.com/titan-engine/titan/issues/18) remain separate proposals.
