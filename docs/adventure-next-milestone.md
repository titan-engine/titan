# Proposal: a third cooperative adventure room

This is the planning outcome of [#87](https://github.com/titan-engine/titan/issues/87).
**The next milestone and every candidate below require maintainer selection.**
Letters identify candidate issue specifications for review, not issued work or a
second execution backlog. Merging this document approves no gameplay expansion,
shared engine implementation, follow-up issue creation or release. After review,
record the selection in #87, create only agreed bounded issues with actual
prerequisites, and replace candidate references with those decisions and links.

Recommend one additional, separately selectable room that tests a different
cooperative decision using the existing Jumper and Strong abilities: Strong
positions a block on a weight plate while Jumper reaches a raised character
plate; simultaneous activation permanently opens one gate for both to exit.
Unlike the existing hold-and-exchange rooms, this tests combining two conditions
and remembering their success. This is a proposed puzzle hypothesis, not a
verified layout or evidence of greater enjoyment. Retain the original two-room
sequence and all its solution routes as the teaching/regression baseline.

Pair that room with game-local Rust room definitions, narrow spatial feedback,
and independent evaluation. Keep movement tuning, additional abilities, moving
cameras, persistent saves, imported art and audio out of the recommended core.
Those are assessed below, with bounded alternatives where there is a concrete
next experiment. Do not combine all directions into a campaign milestone.

## Evidence and limits

Reviewed on 2026-09-06 from main revision
`3e584a023d346805c23c2bf11162c87dee5042ed`, containing completed prerequisites
#80–86 and the shared #95 procedure. Finished-slice runtime evidence was produced
at gameplay revision `02272893a0d91af2b1ac6b5159644b70ab46108c`; PR #122 added
verification and an authoring pointer without changing production gameplay.
The [independent final review](https://github.com/titan-engine/titan/pull/122#issuecomment-5559135056)
reproduced the new semantic runner and disposable variation. This proposal
reviews retained evidence; it adds no new gameplay, GPU or human playtest claim.

| Preceding work reviewed | Outcome and implication |
| --- | --- |
| [#80](https://github.com/titan-engine/titan/issues/80), [PR #101](https://github.com/titan-engine/titan/pull/101) | [Selected design](../games/adventure/design.md): two safe-floor rooms, complementary jump/push abilities, noncolliding characters, fixed view, explicit restart. Room 2 integrates abilities into the same door exchange; it is not a fundamentally different puzzle. Numeric controls and camera remain prototype defaults. |
| [#81](https://github.com/titan-engine/titan/issues/81), [PR #105](https://github.com/titan-engine/titan/pull/105) | Public-API standalone native/headless/actual-WASM game, input filtering, inspection, replay, fixed-aspect rendering. Extend these boundaries rather than creating a new host/framework. |
| [#82](https://github.com/titan-engine/titan/issues/82), [PR #110](https://github.com/titan-engine/titan/pull/110) | Swept movement/support, distinct jump heights and defensive whole-room recovery; 34 movement scenarios/1,363 native/WASM states. Changes to speed, jump or support must recheck ability gates, including the narrow initial-block exclusion. |
| [#83](https://github.com/titan-engine/titan/issues/83), [PR #112](https://github.com/titan-engine/titan/pull/112) | Grounded character plates, OR-controlled door with safe obstruction, both complete footprints at exit; 24 scenarios/1,646 states. Existing blocks do not press plates. New weight/latched devices must have separate rules, preserving old device behavior. |
| [#84](https://github.com/titan-engine/titan/issues/84), [PR #115](https://github.com/titan-engine/titan/pull/115) | Strong-only atomic rail pushes, support/obstruction/rejection reasons; 27 scenarios/2,650 states. Both one-push and two-push solutions are valid. Do not force the suggested two-push route through an artificial socket condition. |
| [#85](https://github.com/titan-engine/titan/issues/85), [PR #121](https://github.com/titan-engine/titan/pull/121) | Explicit Start/Continue/Play again, phase-aware replay and input/capture reset; six sequence scenarios/11,487 states. Preserve this short sequence while testing an optional third room. |
| [#86](https://github.com/titan-engine/titan/issues/86), [PR #122](https://github.com/titan-engine/titan/pull/122) | [Finished evaluation](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/games/adventure/evidence/playtest-86/README.md): three perturbed scenarios, 34 checkpoint recording replays, native Metal and actual browser WebGPU/WebGL2 evidence, unfamiliar plate variation. Findings below bound the next experiment. |
| [#95](https://github.com/titan-engine/titan/issues/95), [PR #109](https://github.com/titan-engine/titan/pull/109) | [Iteration procedure](agent-iteration.md) and [historical skeleton notes](evidence/agent-iteration/adventure-notes.md) separate command phases from full authoring time. Skeleton measurements do not establish finished-puzzle iteration speed. |

The #86 evaluation passed native Metal presentation (4,311 frames), 209 actual
browser checks per backend, and fresh movement/puzzle/block/sequence semantic
conformance. Known-state images showed distinguishable partners, device links,
controls and both exit indicators. Complete solutions used automated input/replay;
direct UI interaction was a short Start/switch/pause/restart check. This establishes
bounded deterministic behavior and platform integration, not human learning time,
accessibility, enjoyment, comfortable jumping or exhaustive freedom from softlocks.

Observed friction was modest but concrete: the oblique fixed view requires
judgement of jump gaps and push stance; text was small in the narrow in-app
viewport and readable in the larger tested viewport; switching requires release
and repress to continue an already-held direction. No production gameplay defect
was found. The terminal block socket cannot reverse because the stance is inside
the ledge, but completion remains possible and R restores it. Leaving B too early
is recoverable by returning to B. Missed jumps land safely. None of these is
evidence that lives, mid-room checkpoints or a pull ability are required.

The [unfamiliar variation](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/games/adventure/evidence/playtest-86/variation-notes.md)
shifted room 1's B plate south 600 mm and completed an adapted 579-tick route,
checked room 2 unchanged, rejected invalid recording without mutation, replayed
selected final-state fields and cleaned up its owned host. Source search found
`src/game/puzzle.rs` after a failed path guess; the guide now points there.
This supports discoverability and a small geometry edit, not easy construction of
a new multi-device room. Native/browser harnesses also repeat some host-specific
setup/assertions while sharing JSON routes; that is not proof all harness logic
should become one shared API.

Full handoff-to-verification monotonic time was missed in that fresh variation.
The 244-second observed UTC interval and 15.044 seconds of first successful command
phases are different, partial measurements. A failed detached launch makes cache
warmth uncertain. No full-task latency improvement or hot-reload need follows.
The [factory proposal](factory-next-milestone.md) independently reports a missing
full variation timer and duplicated local recipe assumptions; these are useful
cross-game inputs to #97, not a common scene or gameplay framework requirement.

## Selection across the requested directions

P1 marks recommended core, P2 supporting or alternative scope, P3 deferred work;
priority does not imply approval or a blocking relationship.

| Direction | Recommendation and evidence boundary |
| --- | --- |
| Puzzle variety | **P1:** one optional room with a block-only weight plate and a raised character plate feeding one permanent two-input gate. Test simultaneous conditions and latched progress rather than repeating another hold exchange. First prove a layout fits the same 12×8 m safe-floor footprint; interest/clarity remains a hypothesis. No timers, pits, enemies, procedural campaign or puzzle graph language. |
| Abilities | **P3:** keep high jump and rail pushing. Evaluate their use in the new room before adding pull, carry, dash or extra characters. The final socket is recoverable, so pulling is not a defect fix. An optional controls experiment below can test directional carry after switching without adding an ability. |
| Checkpoints and save/load | **P2 alternative:** a room-entry resume record is the smallest product experiment; an explicit mid-room snapshot supplies stronger #3 serialization evidence. Recommend neither for this short core milestone. Restart already reconstructs the room; there are no designed pits or measured long-session needs. Recording/replay is not a save format. |
| Camera evolution | **P2 narrow support:** keep fixed framing, add a contextual ground/stance marker and check view occlusion. Do not infer a tracking/orbit camera requirement from one oblique view. A movable camera would also require selected input remapping, inactive-partner visibility and capture/replay policy. |
| Sound and art | **P3:** keep generated primitive art and non-color symbols. Use #2 for demonstrated text/layout gaps and #10 plus the completed sound proposal #37 for an approved asset/audio exercise. Audio is not implemented; a cue cannot be an acceptance dependency until its playback/lifetime boundary is selected. No full animation, mesh-import or music pipeline. |
| Reusable room construction | **P1, game-local:** one explicit Rust room definition consumed by geometry, rendering and inspection for the three rooms, plus validation and authoring docs. Keep mutable state separate and assert literal expected results independently. New geometry is a stronger authoring exercise than relocating one plate. No scene file, editor, generated gameplay source or generic puzzle framework. |

## Candidate issue specifications for maintainer review

A–D are the recommended core. E and F are alternatives requiring an adjusted
validation candidate; they are not additions implicitly approved with A–D.
All inherit the merged first-slice baseline, reference preservation, public API
boundaries and [quality gates](verification.md). Technical dependencies
below apply only if selected. No candidate is blocked on an umbrella proposal.

### A — Specify one simultaneous-condition room (P1, game design)

Outcome: dimensioned layout, device contract and ordinary-input solution for one
optional room, selectable from Start/practice without changing the original
Start-to-two-room sequence. Keep the 12×8 m continuous floor, two characters,
existing movement values, one constrained block and fixed camera. Proposed new
rules: only the grounded block at the declared plate position activates weight
plate W; a grounded character activates raised plate H; W AND H latches gate G
open permanently until room reconstruction. Neither character can substitute for
the block on W. Existing OR doors and character plates remain unchanged.

Acceptance: specify plate containment/support tests, tick ordering, latched-state
inspection and non-color links; G stays solid until activation and never closes
afterward. Define block sockets/stances, geometry, start/exit, both-character
completion, selection/restart/replay behavior and a bounded solution below the
existing recording limit. Prove Strong must move the block at least once and
cannot reach H; Jumper cannot activate W or bypass the full-height gate. Check
jump reach using body/support edges, and inspect whether all relevant elements
fit the fixed view. Enumerate failed jumps, occupied/rejected pushes, block
positions before activation, simultaneous activation, release after latching,
restart and completion. Require a recoverable route or explicit room restart
from every enumerated block arrangement. This is bounded case analysis, not an
exhaustive softlock proof.

Prerequisites: maintainer selects the new device rules; none beyond merged slice.
Risks: permanent opening may remove too much cooperation, or a geometry shortcut
may trivialize a role. Decision: approve this test of simultaneous conditions and
latched progress, revise its contract, or defer content. If a meaningful layout
cannot fit, return for selection rather than expanding the map or adding abilities.

### B — Author the new room through game-local definitions (P1, game implementation)

Outcome: implement A and consolidate immutable room geometry/device definitions
within the adventure package, leaving simulation state and host adapters separate.
One Rust source of truth supplies solids/supports, rendered geometry and inspectable
bounds. Retain game-local device semantics; no new shared Titan gameplay API.

Acceptance: validate unique device identifiers, valid bounds/support heights,
rail sockets/stances and declared links, with useful author-time errors. Migrate
the two old rooms without changing their states, routes, capture expectations or
recording compatibility. Add a bounded, versioned recording representation for
the new room only as necessary; reject unknown rooms/versions before mutation and
retain supported old recordings. Test W/H separately and together, activation
ordering, latch persistence, ability/bypass negatives, all push rejections, both
exit footprints, frozen completion, restart/fall reconstruction, input gates and
capture generation. Compare complete per-tick native/actual-WASM semantic states
against independent expected checkpoints and replay. Add a guide recipe for
changing geometry and constructing a bounded room, identifying definitions,
validation and tests without private source knowledge.

Prerequisite: A's accepted contract. Risks: a definition abstraction can hide
special rules or make tests share the same wrong constants. Keep literal expected
coordinates and outcomes in independent fixtures; extract only fields actually
used by these rooms. Decision: accept local Rust authority; any scene format or
shared primitive requires separate #17/#18 selection, not expansion of B.

### C — Clarify spatial actions in the fixed view (P2, recommended game-local support)

Outcome: display a ground-projection marker for the active airborne character
and a contextual valid push-stance marker when Strong is near the block, using
existing primitive rendering. Keep actual support and push validation authoritative;
these are cues, not aim assistance, snapping or collision changes. Shorten local
text where it improves fit. Do not change movement, jump or switch policy.

Acceptance: derive markers from actual feet, floor and public stance geometry;
show unavailable/occupied action state with shape or text as well as color and
avoid falsely promising a successful landing. Check supported/airborne positions,
all sockets, invalid stance, occupied block and switch/restart. Verify read-only
queries/rendering do not advance state. Inspect actual native and browser captures
at known ticks at 960×540 and 1280×720, plus smaller-window size hints, with both
partners/device cues visible. Keep the fixed camera and 16:9 viewport; distinguish
per-backend visual evidence from portable exact semantic assertions.

Prerequisites: merged #84–86; can proceed independently of A/B. B integration is
checked in D. Risks: markers add clutter or imply reach guarantees. Decision:
approve these two cues or omit C; select a separate bounded camera comparison
only if the evidence still demonstrates occlusion/landing ambiguity. Historical
reference images and the crisp repository README preview remain unchanged.

### D — Evaluate the selected room and authoring workflow (P1, independent verification)

Outcome: evidence for whether a different puzzle is understandable and room
construction is tractable, with negative results retained. Prerequisites: B and
C if selected. The maintainer's interest/clarity judgement is distinct from CI.

Acceptance: independent evaluator completes the old and new rooms through actual
native/browser players; retains ordinary-input solution/replay, failed activation,
missed jump, rejected push, restart and known-state captures. Compare full semantic
states in native/actual-WASM and separately inspect Metal, browser WebGPU and
WebGL2 presentation. State exactly which interactions were scripted, fixture-based
or directly played. Use no source-guided route as evidence of unprompted discovery.
If a human participant is available with maintainer agreement, record first-use
confusion and assistance separately; otherwise report human usability unmeasured.

An unfamiliar agent makes a bounded geometry variation of the new room using
public APIs and local docs in a disposable checkout. State the task and expected
outcome before editing. Start the monotonic timer at handoff and stop at verified
result; record reading/search/editing/failures/build/run/replay/capture phases,
cache/profile/environment, concurrency, interventions and cleanup separately,
following #95. Retain patch, solution, invalid-operation diagnostic and evidence
of unchanged state before a successful repair. Check original rooms unchanged.
If timing is missed, label it unmeasured and rerun a fresh unfamiliar exercise
before claiming a full-task measurement; a repeat by the now-familiar agent is
not equivalent. No one-sample latency budget or hot-reload claim.

Run applicable workspace/game gates and required CI with bounded diagnostics.
Record exact authored/game revisions, capture identity and limitations. Risks:
familiarity, automation and novelty can bias evaluation. Decision: maintainer
judges whether the room warrants further content, changed controls or improved
camera work; passing engineering checks authorizes none of those automatically.

### E — Test a minimal resume boundary (P2, persistence alternative)

Outcome: if persistence is the product priority, choose **one** of E1/E2, starting
with the unchanged two-room slice. E1 is the recommendation for a user-facing
resume experiment; E2 provides broader serialization evidence for #3.

- **E1, room-entry resume:** one explicit versioned export/import containing the
  unlocked/current room entry only. Resume reconstructs that room from its initial
  state and pauses, explicitly telling the player in-room progress is not saved.
  Define completion/Play again and Start behavior; no autosave, cloud or arbitrary
  file writes. Test room 1, room 2, completion, unknown room/version, malformed or
  oversized data, same-session/fresh-process import, input clearing and host/capture
  identity. Invalid data leaves gameplay unchanged. Native/browser exports agree.
- **E2, mid-room snapshot:** one explicit game-owned bounded snapshot covering
  room/phase, both character positions/velocities/support, active selection,
  block position, plate/door/completion state and simulation tick. Define which
  device fields are derived and validate consistency before installation. Rebuild
  ECS handles/UI/rendering; clear pending input/replay, pause and retain live host
  identity while invalidating prior captures. Round-trip airborne/inactive states,
  block support, open-obstructed door, room completion and reset; compare subsequent
  per-tick native/WASM traces with uninterrupted simulation under the specified
  fresh-input policy. Reject invalid bounds/versions/occupancy/state atomically.
  No universal ECS serialization, reflection coupling or compatibility promise.

Prerequisites: merged slice and explicit E1/E2 selection; independent of A–D and
#3 implementation. Follow [save/load boundaries](save-load.md). Acceptance also
requires independent actual-player export/import and a timed unfamiliar author
exercise appropriate to the selected format using D's evidence procedure.
Risks: E1 may disappoint players expecting exact resume; E2 greatly increases
validation/state coverage. Decision: select product resume or serialization depth,
format/size bounds and storage UI before implementation. Including room 3 later
requires explicit coverage of W/H and latch state rather than assuming it works.

### F — Compare directional switching policies (P2, controls alternative)

Outcome: a disposable, bounded comparison of current release/repress movement
with carrying a held direction to the newly active character on the tick after
switch. Keep the switch tick motionless and preserve jump edges and push
release/repress gating after success; inactive
characters still receive no horizontal input. This tests a concrete extra gesture
observed in #86; it does not add abilities or approve changing shipped defaults.

Acceptance: state both policies and task routes first; exercise aliases, airborne
switching, repeated Q, held jump/E, unrelated fresh directions, focus/pause,
restart/transitions and exact keyboard/injected/native/WASM agreement. Record
misdirected actions, release gestures and subjective control feedback separately
from deterministic correctness. Retain captures/replays and disclose evaluator
familiarity. Seek human first-use feedback if available, otherwise leave comfort
unmeasured. Define recording policy/version implications before adopting a winner.

Prerequisites: merged slice only. Risks: carry can cause accidental motion near a
ledge or block; one evaluator cannot settle comfort. Decision: choose this study
instead of more content if controls are the product priority; maintainer selects
whether any policy change becomes a separately bounded implementation issue.

## Shared needs and coordination

The following existing proposals own broader decisions. Their status does not
change through this plan, and they are related context rather than blockers.

| Existing work | Concrete connection and boundary |
| --- | --- |
| [#2 UI layout/typography](https://github.com/titan-engine/titan/issues/2) | Adventure small-viewport text and factory dense native text motivate a selected readability exercise if local wording/cues do not suffice. Do not infer a widget, font or accessibility rewrite from screenshots. |
| [#3 save/load](https://github.com/titan-engine/titan/issues/3) | E supplies a game-owned persistence case; factory candidate G supplies active production/slot state. Select one concrete consumer first; coordinate transient-host boundaries without making reflection universally serializable. |
| [#10 asset formats](https://github.com/titan-engine/titan/issues/10), [#37 sound design](https://github.com/titan-engine/titan/issues/37) | The [existing audio proposal](audio-exercise.md) selects an RPG pickup cue as a proposed first exercise. Reuse its lifetime/device-free/browser concerns if audio is selected; do not duplicate it with an adventure audio subsystem. Adventure event-to-cue choice and readable primitive art remain game-local. |
| [#17 authoring authority](https://github.com/titan-engine/titan/issues/17) | B uses Rust as the sole geometry authority. Three local rooms do not establish a need for scenes, generated source or an editor. Bring measured authoring limits to this proposal before adding another authority. |
| [#18 framework boundaries](https://github.com/titan-engine/titan/issues/18) | Repeated host plumbing may support shared contracts; character abilities, puzzle devices and factory recipes/logistics remain game-local. No general camera/controller/room framework follows from one adventure. |
| [#14 host customization](https://github.com/titan-engine/titan/issues/14), [#19 discovery](https://github.com/titan-engine/titan/issues/19) | Adventure's live native control and owned-instance harness fixes are useful comparison evidence for factory's separate headless tooling. Do not reopen the fixed harness race or infer a discovery-protocol defect. |
| [#15 latency](https://github.com/titan-engine/titan/issues/15), [#16 hot reload](https://github.com/titan-engine/titan/issues/16) | Both game variations missed full-task timers. D repairs measurement, not engine speed; #95 phases and factory fixed-grid command timings cannot identify a compile/reload bottleneck. |
| [#96 repair guidance](https://github.com/titan-engine/titan/issues/96), [#97 shared review](https://github.com/titan-engine/titan/issues/97) | Supply #86's intentional invalid-recording diagnostic/recovery and authoring findings to existing shared assessments. Their implementation/selection is independent; do not duplicate shared diagnostics or make either game plan an artificial prerequisite. |

The [factory recommendation](factory-next-milestone.md) also retains its original
challenge, tests one new mechanic, keeps content definitions local and demands
honest fresh-author timing. Share the evaluation procedure and demonstrated host
contracts; preserve separate puzzle/routing acceptance and ownership. Coordinate
any eventual shared-file implementation under one approved owner after #97 review.

The important maintainer decisions are: select A–D (optionally omit C), E1/E2,
F or a revised milestone; approve the proposed permanent two-input gate and
optional-room entry; retain current abilities/input policy and fixed camera unless
an alternative is explicitly selected; choose whether human first-use evaluation
is available; and select any shared UI/audio/save/host work separately. Recommend
A–D with Rust room definitions and the two narrow visual cues. No issue creation,
implementation, queue submission of follow-up work or release follows from this
recommendation. The [vision](vision.md), [requirements](design-requirements.md)
and [workflow](workflow.md) continue to govern those decisions.
