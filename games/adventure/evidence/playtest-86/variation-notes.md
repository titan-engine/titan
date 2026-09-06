# Independent puzzle variation, issue #86

Unfamiliar evaluator at source `02272893a0d91af2b1ac6b5159644b70ab46108c`.
Task stated before editing: shift only room 1's far plate B south 600 mm,
complete with ordinary input, reject an invalid recording without mutation,
and reproduce completion through semantic replay. Production gameplay was untouched.

The public `plates(room)` API was found by source search. A first guess at
`src/puzzle.rs` failed; the declarations live in `src/game/puzzle.rs`.
The README explains the mechanics and input filtering; the public solution
fixture and `scripts/test-control.py` supplied route and CLI-envelope examples.
No private implementation hints or human interventions were used. An explicit
README authoring pointer to the geometry module would reduce discovery work.

The variant passed: A held by Jumper, relocated B held by Strong, door open
during exchange, both footprints grounded in the exit. The adapted 579-tick
route adds ten south ticks after Strong crosses and ten north ticks at the end.
The emitted recording replays after restart with exact equality for characters,
selection, session tick, consumed input, puzzle, room, phase and block. An invalid
fixture returned `invalid_value`; the complete queried state stayed unchanged.
The diagnostic bundle and API text were read. Room 2's far plate was also checked
unchanged. Both successful runs shut down their owned host and verified discovery
cleanup. No GUI was launched.

A detached subprocess launch did not persist beyond its tool call; the attached
run succeeded. This makes the first successful build's cache condition uncertain:
the isolated target began empty but could have been partly populated. First
successful measured phases were 4.807287375 s CLI build, 5.030782750 s game build,
and 5.205637500 s runtime. Strengthened assertions passed again with warm builds.
The first observed UTC-to-verification interval was 244 seconds. **Full handoff
monotonic timing was missed**, so the baseline honestly leaves full task time
null; it does not equate command timing with full authoring time.

This completed variation is historical. Its [runner and required route](https://github.com/titan-engine/titan/blob/3e584a023d346805c23c2bf11162c87dee5042ed/games/adventure/evidence/playtest-86)
remain at evidence revision `3e584a023d346805c23c2bf11162c87dee5042ed`, separate
from the measured source and applied patch. Reproduce from the repository root
using external disposable directories:

```sh
scratch=$(mktemp -d)
evidence_root=$(mktemp -d)
git archive 3e584a023d346805c23c2bf11162c87dee5042ed games/adventure/evidence/playtest-86 | tar -x -C "$evidence_root"
evidence="$evidence_root/games/adventure/evidence/playtest-86"
git archive 02272893a0d91af2b1ac6b5159644b70ab46108c | tar -x -C "$scratch"
patch -d "$scratch" -p1 < "$evidence/variation.patch"
python3 "$evidence/variation-reproduce.py" "$scratch" "$scratch/results"
```

The script bounds builds at 1200 seconds, commands at 60 seconds, host lifetime
at 120 seconds, and shutdown at 15 seconds. It creates an isolated target and
selects project and instance explicitly. The route, exact patch, recording,
compact runtime results, API diagnostic summary and baseline are retained here.
Only CPU behavior and listed final-state replay fields are claimed; no per-tick
replay, WASM/GPU, visual, or portable performance claim is made.
