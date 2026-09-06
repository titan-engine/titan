# Adventure initial iteration evidence

The historical [executable procedure](https://github.com/titan-engine/titan/blob/1b1f138da009e589521df7d3e155e711562a8375/docs/evidence/agent-iteration/adventure-baseline.py) runs the committed `HEAD`
in a disposable source copy with an empty Cargo target. It uses native CPU
hosts, authenticated CLI discovery and no GUI focus. First prepare the pinned
[disposable reproduction checkout](README.md#reading-the-numbers); from its root:

```sh
mkdir -p target/evidence
python3 docs/evidence/agent-iteration/adventure-baseline.py > target/evidence/adventure-result.json
```

Run with normal Python assertions enabled. Cargo, a Rust toolchain, Python 3.9+
and the dependency cache or network access are required. Any Cargo fetching is
included in build times. The repository `scripts/acceptance_process.py` helper applies build/runtime
timeouts, inherited acceptance deadlines, SIGTERM handling and owned process
group/discovery cleanup. `TITAN_BUILD_TIMEOUT_SECONDS` and
`TITAN_RUNTIME_TIMEOUT_SECONDS` configure its bounds (recorded in final JSON);
hosts additionally have a 120-second application runtime bound. This is a fixed
`adventure-v1` skeleton exercise, not a benchmark platform or a test of later
puzzle mechanics. Later verification must adapt assertions to its pinned
revision and record the changes. Uncommitted gameplay changes are excluded by
`git archive HEAD`; this evidence script itself need not be committed to run.

Each timed phase starts immediately before its Python function and ends after
its assertions. Times include CLI subprocess startup and JSON parsing. Setup,
source archive extraction, this agent's reading and script authoring, and final
cleanup are excluded. The replay timer starts after recording retrieval; the
scenario timer starts with a running, inspected host. The rule timer includes
editing the scratch constant, incremental build, launch/discovery and assertion.
Initial CLI and game builds share the isolated target in that order, so the game
build reuses dependencies produced by the CLI build. Cargo registry/download
cache is shared with the machine. Concurrent CPU activity from other agents is
uncontrolled; neither build reported a Cargo lock wait. These are individual
observations with no latency budget or cross-machine performance claim.

The scenario sends complete future-frame snapshots: right; right+switch;
release all; up. Jumper ends at `(1560,6500)`, Strong at `(3500,6440)`, Strong
selected. Switching consumes its tick without moving either character. The
recording is replayed and compared on characters, active character, session tick
and consumed input. Host frame and reset generation are intentionally excluded
from replay equality because replay reconstructs the session.

The rule exercise changes only `AXIAL_STEP` from 60 to 90 in the scratch source,
rebuilds and launches a fresh host, then verifies one right tick moves Jumper
from X=1500 to X=1590 while Strong stays still. Diagonal speed is deliberately
unchanged; this is an axial-rule experiment, not proposed gameplay tuning.
The scratch copy is discarded and current gameplay is never edited.

The failure exercise deliberately changes the recording's fixture to `wrong`.
The host returns `invalid_value` with “unsupported, truncated or oversized
recording”; a subsequent query must equal the complete pre-request state.
The final script also reads the diagnostic manifest, checks that its request
contains the wrong fixture and its game snapshot equals the pre-request state,
and verifies `api.txt` exists and no capture was included. It then submits
the original valid recording and verifies the four replay-equivalence fields,
so diagnosis includes a successful recovery; reset generation may advance. The generic error
message alone does not distinguish the bad-fixture case; request evidence plus
`game::replay` validation identifies this known injected cause. Local bundle
paths and discovery tokens are not copied into the public result.

CPU capture returns `unsupported` and produces no image. Native GPU and browser
players have capture workflows documented in the [game guide](../../../games/adventure/README.md),
but this run does not claim either was exercised. Jumping, blocks, plates,
doors and completion are absent at this skeleton revision. Construction here
means a two-character input scenario; there is no teleport or writable position
field, and the procedure checks field metadata explicitly.

## Attempts, documentation and intervention log

- One setup attempt failed before any timed phase: Python 3.9's `tarfile` does
  not support the newer `filter` argument. The script was corrected to extract
  its trusted local Git archive with the Python 3.9 API. No game failure occurred.
- [Initial measured run](adventure-initial.json) passed every timed phase.
  [Diagnostic verification run](adventure-diagnostics.json) repeats the exercise
  after adding diagnostic request/state/API assertions. The
  [cleanup verification run](adventure-cleanup.json) additionally uses subprocess
  groups so timeout cleanup also terminates Cargo descendants. The
  [recovery verification run](adventure-recovery.json) adds valid-recording
  recovery after the deliberately rejected replay. The
  [final verification run](adventure-verified.json) uses the repository acceptance
  process helper for standard timeout overrides and signal cleanup. All five
  runs are retained; these verify stronger procedures, not selectively faster runs.
- Each completed run contains one attempt per phase, zero unexpected runtime
  failures, and two intentional rejection probes (bad replay and CPU capture).
  Zero human interventions were needed. Rule edit and setup repair were agent
  actions, not human assistance. No GUI/native focus intervention was required.
- The game README supplied launch, control, input, position units, capture limits
  and recording semantics. It has no complete CLI `invoke replay` example;
  `scripts/test-control.py` supplied the nested
  `{"recording": ...}` arguments shape. This procedure supplies that runnable
  example. The README also has no speed-tuning source location; the agent used
  `rg AXIAL_STEP games/adventure/src` to identify the constant. Neither gap
  blocked execution. No unexplained gameplay or build failure was encountered.

Use the JSON revision, UTC start, environment, cache/boundary text, per-phase
wall times, assertion evidence and attempt log together. `unexpected_failed_attempts`
in each result counts that measured run; it does not erase the setup failure
above. No end-to-end human-request-to-answer or agent reasoning-time number was
measured, so these data cannot establish overall agent productivity.
