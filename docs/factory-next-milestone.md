# Proposal: a branching factory challenge

This is the planning outcome of [#94](https://github.com/titan-engine/titan/issues/94),
for maintainer selection after the completed first slice. **No next milestone or
candidate below is approved.** Candidate labels are review references, not issue
numbers or a live execution backlog. Once selected, record the decision in #94
and create only the agreed bounded issues on the project board; those issues own
status, assignment and implementation. Supersede this proposal with the decision
and links rather than maintaining parallel checklists here. Integration of this
document approves neither gameplay expansion nor a release.

Recommend one second challenge on the existing 12×8 grid: split the existing ore
supply between two processors, merge their plate outputs using ordinary conveyor
inputs, and deliver ten plates. Preserve the first challenge as a selectable
regression/tutorial. Pair this with clearer repair feedback and a small throughput
readout so a player can explain why parallel processing helps. Keep the existing
ore-to-plate recipe and free construction. This tests a new routing decision while
holding recipes, economy and world size constant. The claim that it is more engaging
is a hypothesis to test, not a finding of the first playtest.

## Evidence reviewed and what it establishes

GitHub prerequisites #88–93 and shared baseline #95 were CLOSED when reviewed on
2026-09-06. This proposal starts at merged verification revision
`17723e62334a19763f8cf81b2f31cc840b4d6289`, containing their merged work. The
finished-game observations themselves are pinned to
`e4800939606889669e8a9b04650cda4bce6df37d`; merging its evidence did not change
production source. See the [final independent integration review](https://github.com/titan-engine/titan/pull/114#issuecomment-5558667704)
and [verification report](evidence/factory-verification/README.md).

| Preceding work | Reviewed outcome and implication |
| --- | --- |
| [#88](https://github.com/titan-engine/titan/issues/88), [PR #100](https://github.com/titan-engine/titan/pull/100) | [Selected rules](factory-slice.md): one deposit, one output per conveyor, snapshot-empty capacity, positional contention priority, explicit discards, ten-plate completion. Ordinary belts already merge; they cannot split. Fairness is not guaranteed. |
| [#89](https://github.com/titan-engine/titan/issues/89), [PR #107](https://github.com/titan-engine/titan/pull/107) | Standalone public-API game, camera/grid construction and validated ordered operations in native/headless/WASM. These are the extension boundary, not a new editor requirement. |
| [#90](https://github.com/titan-engine/titan/issues/90), [PR #108](https://github.com/titan-engine/titan/pull/108) | Deterministic corners, competing feeds, congestion, cycles and occupied edits. A splitter must extend destination selection without silently changing existing merge priority or snapshot semantics. |
| [#91](https://github.com/titan-engine/titan/issues/91), [PR #111](https://github.com/titan-engine/titan/pull/111) | Complete ore-to-plate loop; reference first delivery 189, subsequent interval 120, completion 1269. Extraction is every 60 eligible ticks, so two processors can plausibly use supply currently backed up behind one processor. Exact new routing timing remains to be specified. |
| [#92](https://github.com/titan-engine/titan/issues/92), [PR #113](https://github.com/titan-engine/titan/pull/113) | Palette, previews, slot/progress details and shared read-only explanations. Extend those surfaces for routing; no generic widget system is required by this challenge. |
| [#93](https://github.com/titan-engine/titan/issues/93), [PR #114](https://github.com/titan-engine/titan/pull/114) | Independent finished-slice native/browser construction, diagnosis, edits, completion and reset; unfamiliar variation and bounded larger fixtures. Findings below constrain this recommendation. |
| [#95](https://github.com/titan-engine/titan/issues/95), [PR #109](https://github.com/titan-engine/titan/pull/109) | [Shared procedure](agent-iteration.md) records revisions, attempts, timing boundaries and cleanup. Its construction-skeleton measurements are historical, not final production measurements. |

The [player exercise](evidence/factory-verification/player/README.md) repaired a
wrong-facing line in both actual players. Its separate wrong-item exercise first
failed because installing a processor left ore queued downstream. Explicitly
clearing the four downstream belts and replacement tile discarded five ore;
later removing an occupied processor discarded two more. The recovered browser
run completed at 1926 with 22 extracted, seven discarded and five resident items.
[Native parity](evidence/factory-verification/player/native-browser-traces.json)
checks 3,767 conserved boundaries and seven matching semantic checkpoints. This
is evidence for clearer cleanup instructions, not automatic item conversion.

Observed interface limits: dense native all-caps text, native restart resuming
while browser restart pauses, no individual native accessibility controls and no
live native-player discovery/inspection. The separate headless CLI host does
provide tooling; fixture stdout and screenshots supplied native player state.
The evaluator knew the documentation and acceptance source: this was independent
engineering playtesting, not blinded human usability or a screen-reader study.
No silent loss or simulation defect was found in the bounded exercised scenarios.

The [unfamiliar-agent variation](evidence/factory-verification/variation/README.md)
changed processor work from 120 to 90 ticks in five source lines across simulation,
metadata and interface. It verified first delivery 159, completion 969, 978
conserved boundaries, exact replay and captures. Source search exposed duplicated
recipe assumptions. This supports consolidating recipe configuration if more
recipes are selected; changing a duration does not prove new item types or recipes
are easy. The original 120-tick suite was not updated/passed for that scratch
variant. Its full receipt-to-result timer is unavailable; the 324.015-second
partial wall interval excludes the first read and final cleanup and is not a
speedup benchmark. A failed host-frame freeze assertion was an evaluator error.

[Scaling results](evidence/factory-verification/scaling/results.json) and the
[measurement boundaries](evidence/factory-verification/scaling/README.md) cover
three repeats per fixture, all within the same 96-cell world:

| Fixture | Structures / connections | Resident items start → end | Seconds for 600 public one-tick calls (range of three) |
| --- | --- | --- | --- |
| Reference active | 10 / 9 | 5 → 5 | 0.349–0.369 |
| Long active | 49 / 47 | 10 → 15 | 2.207–2.322 |
| Dense active | 96 / 81 | 10 → 15 | 5.837–5.895 |
| Dense stalled | 96 / 81 | 50 → 50 | 7.583–7.807 |

The long path has 42 processor-output hops; dense adds 47 starved filler
processors, not more production sources. All repeats conserved independently
counted items and compared full states. The saturated run retained 50 items and
unchanged machine/counter state from ticks 12000–12600. Two aggregate timeouts
(60 and 180 seconds) preceded a completed 273.998-second subprocess under a
300-second hang bound. Warmup and assertions are outside the 600-call timing.
Calls include parsing, result generation and recording; separate inspection
samples include serialization/parsing/checking, and capture samples use software
rendering/checksums. These shared-host macOS M5 Pro Cargo-dev observations establish
neither scheduler cost, player FPS, portable capacity nor scaling beyond 12×8.
They justify measuring a selected new workload before optimization or map growth.

## Selection across the seven directions

Priorities below rank proposed scope, not approval. P1 is the recommended
milestone core; P2 is useful supporting/alternative scope; P3 is later exploration.

| Direction | Evidence versus hypothesis | Recommendation and boundary |
| --- | --- | --- |
| Branching logistics | One output prevents splitting; 60-tick supply and 120-tick processing give a concrete balancing experiment. Enjoyment and fairness needs are untested. | **P1, select:** one splitter and a two-processor challenge. Reuse ordinary merge inputs; defer dedicated merger, underground belts, filters and global routing. Game-local. |
| More recipes | Duration copies are observed authoring friction; no multi-recipe exercise exists. More content may add decisions but also wrong-item states. | **P2, alternative:** consolidate one local recipe definition, then one additional one-input/one-output recipe only if selected instead of routing. No generic recipe engine. |
| Placement costs | Free removal with explicit item discard supported successful repair. There is no observed economy problem. | **P3, defer:** costs risk making recovery unrecoverable; a later finite-stock challenge must specify refunds, initial stock and a provably recoverable solution before implementation. |
| Progression | Ten plates completes the only challenge. Desire for unlocks/campaign length is unmeasured. | **P3, defer:** select either challenge directly; no unlocks, research, persistent currency or campaign. Evaluate a second challenge before linking a progression chain. |
| Saves | Ordered commands are bounded verification records, not resumable saves; active machines/slots would add meaningful persistence coverage. No long-session need was measured. | **P2, alternative:** one game-owned snapshot round trip, following #3; keep out of the short routing milestone unless resume is the maintainer's product priority. |
| Larger maps | Dense fixed-grid public operations cost more; no larger dimensions or extra extractors were measured. | **P3, defer:** new size, camera readability and multi-source workload need separate acceptance. Do not derive a map capacity or ECS rewrite from the table. |
| Production statistics | Existing counts/status explain instantaneous stalls; no rate comparison UI was tested. Rates may help compare one and two processors. | **P2, select narrow support:** one bounded simulation-tick delivery-rate window plus current counts and stall explanation; no graphs/history dashboard or analytics service. Game-local. |

## Candidate issue specifications for review

All candidates require explicit maintainer agreement before creation/Ready status.
The already-merged first slice satisfies their code baseline. Letter dependencies
are actual technical prerequisites **if these candidates are selected**; priority
alone introduces no dependency. A–E form the recommended milestone. F–G are
bounded alternatives, not extra commitments.

### A — Specify the routing challenge (P1, game design)

Outcome: a reviewed second-challenge contract and one exact solution fixture on
12×8, retaining the original challenge and trace unchanged. Specify deposit and
delivery positions, target ten plates, available buildings, and a layout that
can route through two processors and one ordinary merge. Proposed splitter:
one tile, one rear input slot, outputs straight and clockwise relative to facing;
one transfer per tick, persistent alternating preference after successful sends,
skip an unavailable preferred output, preserve preference on a blocked tick.
Maintain snapshot-empty capacity and existing source `(y,x)` arbitration.

Acceptance/verification: hand-worked expected traces for both outputs free, one
blocked, both blocked, competing destinations, occupied rotation/removal and
restart. Define preference reset/rotation behavior, item accounting, completion
and challenge selection at a safe reset boundary. Prove a bounded two-processor
solution and compare its delivery interval against a one-processor route; include
routing/transient delays, not a promised exact 2× completion improvement.
No requirement that the player use exactly that solution.

Prerequisites: none beyond merged #88–93; maintainer chooses routing as the next
milestone. Risks: small-grid layout may constrain meaningful choice; fallback
routing can complicate contention ordering. Decision: accept the proposed
alternation/fallback semantics and retain fixed-priority merges, or revise the
contract before B. If the layout cannot fit, reduce scope or return for selection;
map expansion is not implicit.

### B — Implement the splitter and second challenge (P1, game-local)

Outcome: both challenges playable through the existing native/headless/actual-WASM
adapters, with a splitter available only in the second challenge. Shared Titan
crates remain consumers of game-defined state/commands.

Acceptance/verification: implement A's exact traces, validated construction,
rotation/removal discard previews, output/preference inspection and bounded ordered
commands. Independently count resident/delivered/discarded items at every boundary;
exercise cycles, contention, branch starvation, fallback and reset after edits.
Compare full native/WASM semantic states for identical operations and repeat
from fresh state. Extend palette, port arrows, hover previews and selected-tile
explanations in both players; inspect actual native/browser images against known
slots, verify focus/camera mapping and player-built completion. Original reference
still completes at 1269. No change to original merge fairness or recipe rates.

Prerequisite: A's accepted contract. Risks: multiple output reservations duplicating
an item, preference advancing on failed transfers, stale UI ports. Decision: any
need for engine API changes requires separately selected scope; no general
logistics framework is authorized by this candidate.

### C — Make recovery and restart expectations explicit (P1, game-local UX)

Outcome: existing wrong-item feedback explains that a newly placed processor does
not consume already queued downstream ore; previews show the cost of clearing it.
Recommend both hosts restart paused, with visible pause state and cleared inputs.

Acceptance/verification: replay #93's wrong-item/occupied-processor repair without
source access; assert five then two ore discards and completion at 1926 for its
unchanged ordered simulation sequence. Verify UI/query remedies agree, inspection
is read-only, and user-visible restart/step/resume in both hosts starts an empty
run without a held click/key leaking. Inspect native text and browser wrapping
at the previously exercised sizes; shorten wording before proposing typography
infrastructure. No auto-purge, refunds or changed conservation.

Prerequisites: merged #92–93 only; independent of A/B. Risks: pause-policy change
may surprise current native users; longer remedies may overflow. Decision:
maintainer accepts paused restart or explicitly selects/document another common
policy. Accessibility-tree integration and live native-player control remain
separate shared-host investigations, not implied fixes here.

### D — Show a bounded production-rate readout (P2, game-local)

Outcome: display cumulative deliveries and a rolling last-600-simulation-ticks
rate, with sample length/startup indication, alongside the existing selected
machine's current stall reason. One bounded counter ring; no per-machine history.

Acceptance/verification: specify numerator/window boundaries and displayed units
(plates per simulation minute), use integer counts and deterministic formatting;
check startup, no deliveries, steady supply, blockage, pause, completion freeze
and reset against independently enumerated delivery ticks. UI and read-only
query must agree without advancing state. Inspect both players with large counts
and sample-length text. Original one-processor trace validates the calculation;
E compares the new challenge. This rate is not wall-clock FPS or machine utilization.

Prerequisites: merged #91–92 only; A/B are not prerequisites to implementing this
readout. Risks: short-window startup and completion can mislead comparisons;
label both explicitly. Decision: approve this single window and minimal display,
or omit D if repair text and counts suffice; do not expand into a dashboard.

### E — Independently evaluate the routing milestone (P1, verification)

Outcome: selection evidence for whether branching adds an understandable decision
and whether it keeps authoring/verification tractable, including unsuccessful
attempts. Prerequisites: B, C, and D if D is selected.

Acceptance/verification: independent native/browser player builds both routes,
blocks one branch, explains fallback/backpressure, repairs and completes; compare
known-state captures to inspected items. Compare single/parallel processing with
identical supply and explicit warmup/measurement boundaries, check determinism
and per-boundary accounting. An unfamiliar agent makes one bounded local splitter
policy or recipe-duration variation in a disposable checkout using public docs;
start a monotonic timer at handoff, retain patch, failures, diagnosis/recovery,
replay and relevant capture. Do not repeat the missing full-time claim from #93.
Repeat bounded active and stalled fixtures within 96 cells, recording environment,
profiles, cache/concurrency, command/inspection/capture phases separately and
retaining partial failures. No performance pass threshold from one machine.
Run applicable [quality gates](verification.md) and game checks, with
actual browser GPU evidence distinguished from Node/WASM parity. Preserve all
existing reference checksums and README art.

Risks: engineering familiarity can bias playtesting; report prior context and
separate observed behavior from enjoyment hypotheses. Decision: maintainer judges
whether this warrants further content; success does not automatically authorize
progression, larger maps or optimization.

### F — Consolidate recipes and add one serial recipe (P2, alternative)

Outcome: if content depth is preferred to branching, retain the original challenge
and add one second challenge that converts ore → plate → gear, each conversion
one-to-one, using a separately configured processor variant. No multi-input
assembly, recipe switching on occupied machines, new deposit, splitter or economy.

Acceptance/verification: first specify gear work duration, ports, target and exact
solution on 12×8. Use one game-local recipe definition for simulation, inspection
bounds and UI progress/text; retain independent literal expected timing tests so
configuration and tests cannot agree on the same accidental error. Assert both
recipes, wrong-type rejection, blocked finished work, occupied removal, restart,
full accounting across item types and native/WASM parity. Player-build both
challenges in native/browser; unfamiliar author changes a duration and records
all required edits plus full handoff timing. Original first-slice traces unchanged.

Prerequisites: no dependency on A–E; merged slice suffices, but recipe timing and
layout need maintainer agreement before implementation. Risks: the five-line
variation did not exercise new types, so schema/UI/test changes may dominate.
Decision: choose F instead of A/B for a bounded content milestone, with C and a
separately scoped equivalent independent evaluation; do not combine both by default.
Keep recipe data game-local; repeated cross-game need would inform #18 later.

### G — Resume one factory run (P2, alternative persistence exercise)

Outcome: one versioned game-owned snapshot with export/import in native and
browser, scoped to the original challenge. Persist tile kinds/facings, slots,
in-process work, extractor progress, counters, outcome/completion and game tick.
Follow the [save/load boundary](save-load.md); reconstruct ECS handles, UI and
rendering, clear pending/held input, preserve live host identity and explicitly
pause on load. The bounded operation recording is not the save format.

Acceptance/verification: bound bytes/counts and validate versions, tile uniqueness,
ports/types, counters/conservation, work ranges and completion consistency before
installing state. Invalid/truncated data leaves live gameplay unchanged. Round-trip
mid-work, blocked, edited and complete states; subsequent native/WASM tick traces
must match uninterrupted runs. Check fresh-process and same-session loads, UI
refresh, camera/input reset policy and exported-data portability across both hosts.
No arbitrary path writes, cloud storage, autosave or compatibility promise beyond
the selected version.

Prerequisites: merged first slice; independent of routing and #3 implementation.
Risks: loading inconsistent but superficially valid state; confusing host/game
clock identity. Decision: approve resume as the product priority and format/size
bounds. If selected after B/F, explicitly re-scope to include splitter preference
or gear state; it must not silently claim to save the extended challenge.

## Shared engine proposals and deferred alternatives

These existing OPEN proposals were inspected on 2026-09-06. None is Ready merely
because the factory slice finished, and none blocks A–E:

| Existing proposal | Connection and next decision |
| --- | --- |
| [#2: broader ECS UI layout and typography](https://github.com/titan-engine/titan/issues/2) | Dense bitmap wording and native accessibility gaps are evidence to bring to a concrete UI exercise. C first tests shorter local wording. Native accessibility integration needs an explicitly bounded host/UI investigation; the browser's DOM controls do not prove native accessibility. No widget/font rewrite follows automatically. |
| [#3: save/load coverage and reflection coupling](https://github.com/titan-engine/titan/issues/3) | G supplies machine/slot persistence missing from arena/RPG exercises. Explicit save types can test that boundary without coupling inspectability to serialization or selecting a universal ECS serializer. |
| [#15: iteration latency and budget policy](https://github.com/titan-engine/titan/issues/15) | E can supply complete authoring intervals, failed attempts and separate phases. Current public-command timings and two aggregate timeouts do not identify engine hot spots or justify CI speed thresholds. Profile only a demonstrated bottleneck in a separately approved investigation. |
| [#18: framework opinion and extension boundaries](https://github.com/titan-engine/titan/issues/18) | The local variation supports discoverable game source and local recipe consolidation. The ECS-only audit already recommends a separately approved external-consumer check; factory routing does not require it. No reusable transport/recipe primitive or crate splitting is justified by this single game. |

A factory live native-player inspection exercise could reuse existing public host
facilities, but first needs an agreed same-process observation/capture contract
and coordination with the adventure/host owner. It is distinct from native
accessibility and neither is a required dependency for a small routing challenge:
#93 already demonstrated bounded verification using player fixtures plus the CLI
host. Do not infer a generic shared API change from that workaround.

Reject a combined routing + recipes + costs + progression + saves + larger-world
milestone: it would change supply, demand, failure recovery and scale together,
preventing a useful comparison with the completed slice. Defer dedicated mergers
because ordinary inputs already merge; defer fair global arbitration until a
concrete challenge needs it. Defer map growth and scheduler/storage optimization
until a selected workload and profiling distinguish simulation from tooling cost.

The maintainer's selection is therefore: approve A–E (optionally omit D), choose
the bounded content alternative F, choose the resume exercise G, or request a
revised proposal. Also decide splitter fallback/fairness, common restart policy,
and whether accessibility/live native inspection merits separately bounded work.
Only then translate selected specifications into issues with acceptance and real
blocking relationships. The [vision](vision.md), [requirements](design-requirements.md)
and [workflow](workflow.md) remain authoritative for shared engine direction and
approval; this proposal adds no second live backlog.
