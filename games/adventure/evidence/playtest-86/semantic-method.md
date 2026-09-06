# Independent semantic playtest for #86

The independent semantic evaluator authored `scripts/test-playtest.py` against
this game's documented native discovery host and public CLI. It reads the
existing solution segments as navigation templates, then adds independently
chosen excursions, grounded pauses, held transition gestures and failed
attempts. It does not claim that the reference routes themselves are newly
discovered solutions. No production code or gameplay mutation was used.

Reproduce from the repository root:

```sh
python3 games/adventure/scripts/test-playtest.py --output games/adventure/evidence/playtest-86/semantic.json
```

The runner builds the native game and CLI, launches three individually bounded
hosts, queries their actual state and recording, and gracefully stops each host.
The evidence JSON records the game source revision, runner SHA-256, UTC start,
input operations, checkpoint state including host frame and session generation,
recording hashes, and cleanup outcomes. The checked-in runner was uncommitted
when the evidence was generated; its hash identifies the exact runner bytes.
Replaying checkpoint recordings compares phase, room, both complete character
states, active character, block, plates/door/exit/completion, room tick, consumed
input, held gates and recovery feedback. Host frames and reset generations are
retained as provenance, not expected to equal the original session on replay.

## Scenarios and observations

- Both complete room-1-to-room-2 sequences use the two-push and intermediate
  one-push route. Extra diagonal/cardinal excursions restore the starting
  location; seven-tick waits at grounded support and plate checkpoints perturb
  timing. All named solution conditions are asserted, including only Jumper in
  the exit before Strong arrives, and both complete footprints at completion.
- Start, Continue and Play again are pressed together with right, jump, switch
  and interact, then held. The destination remains stationary with Jumper
  selected until release; completion also freezes character state and room time.
- After leaving A, the evaluator switches to Strong, abandons B, observes the
  closed door, returns Strong to B, then switches back and completes the route.
  This is a recoverable temporary blockage, not an observed softlock.
- A Jumper push is rejected. A midair switch with movement/jump/interact held
  selects Strong once, stops Jumper's steering while gravity continues, and
  prevents Strong from moving or jumping from the old hold. A fresh unrelated
  down direction immediately works while right remains suppressed.
- Strong's attempted jump onto the moved block fails and lands safely without
  undoing the push. Walking around the east side to the reverse stance restores
  the intermediate block to its original socket. This demonstrates a usable
  ordinary-input recovery path without a reset command.
- Restart while airborne clears scheduled future movement, restores the current
  room and block, and increments generation without advancing the host frame.
  The following empty tick remains at the restored positions.

No production gameplay defect was observed in these bounded scenarios. The
first runner attempt had a test-authoring field-name error: inspection exposes
`block.moves`, whereas the guide describes it in prose as move count. Correcting
the assertion required no game change or human intervention.

## Limits

This is native headless semantic evidence, not browser/WASM parity, GPU capture,
physical-key focus behavior, or a human usability study. The navigation remains
scripted, and timing perturbations do not measure comfort or demonstrate that an
unfamiliar human can discover the puzzle. The release/repress policy is
predictable under these tests but its ergonomic cost needs human feedback.
Checkpoint replay is semantic equality at those checkpoints, not a newly added
per-tick cross-platform trace comparison. No below-floor input can be reached
through the public host: defensive fall recovery requires the existing optional
controlled fixtures and is reported separately by the integration evaluator.
