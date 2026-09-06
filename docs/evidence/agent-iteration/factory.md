# Factory initial agent-iteration measurement

Measured 2026-09-06 at revision
`0468ffe00b2cb109acc33591dc382839196ce7fe`, which contains factory construction
#89 and adventure skeleton #81. This is a headless construction-stage baseline,
not production/transport verification. The game README explicitly states that
advancing time creates no items or deliveries.

[Machine-readable results](factory-measurement.json) preserve the complete rerun
with failure recovery and pre-edit rule probes. The unchanged
[first sample](factory-initial-measurement.json) is retained separately. Both preserve measured values,
metadata, scenario state and operation recording. Run the
[historical measurement script](https://github.com/titan-engine/titan/blob/1b1f138da009e589521df7d3e155e711562a8375/docs/evidence/agent-iteration/factory-measure.py) from the pinned
[disposable reproduction checkout](README.md#reading-the-numbers):

```sh
python3 docs/evidence/agent-iteration/factory-measure.py
```

The script measures the committed `HEAD` through `git archive`, ignoring working
changes. It writes a fresh `factory-measurement.json`, so preserve the initial report
by running in the disposable checkout described in the
[reproduction guidance](README.md#reading-the-numbers), then copying results to
ignored output in the maintained checkout. It uses a disposable source tree and an empty
private Cargo target. Existing global Cargo registry/toolchain caches remain;
this is not a clean-machine install measurement. No native window or browser is
opened. Other agents ran native GPU checks and workspace builds on the same machine;
CPU, registry/cache and filesystem contention were not isolated. No compilation
output directory was shared. macOS 27.0 arm64 on Apple M5 Pro (18 logical CPUs),
Python 3.9.6, rustc 1.98.1 and Cargo 1.98.1 were used. Builds used the default
Cargo dev profile (unoptimized with debug information); the exercised backend
was native headless control with software rendering. No `RUSTC_WRAPPER` was set.

The timer for each phase starts immediately before its automated action and
ends after its assertions pass. It includes process startup, CLI transport,
serialization and assertion overhead. Reading docs, understanding the task,
authoring the harness, extracting the scratch source and deleting its build
artifacts are outside these phase timers. These are two single-run samples with different exercise boundaries, not a
statistical series of execution-to-verification times. They are not end-to-end prompt-to-result agent latency,
percentiles, numerical budgets or claims about a typical developer machine.

The table reports the expanded rerun. The initial sample remains unchanged in
its separate JSON file.

| Exercise / boundary | Seconds | Verified result |
| --- | ---: | --- |
| Build CLI, then headless game, private empty target | 23.459647 | Both binaries built with `--locked` |
| Start paused server → discovery and initial state | 0.240072 | First discovery poll found instance; one fixed delivery, tick 0 |
| Query capabilities, command metadata and entities | 0.022558 | Controlled headless schema 2; inspect/step/invoke/query/capture exposed |
| Write scenario → controlled sequence → state and recording | 0.022216 | Extractor (1,3), conveyor (2,3), processor (3,3), all east; advance 60; four structures including delivery, zero delivered |
| Fresh sequence process → full state equality | 0.006443 | Four successful outcomes; full JSON state equals controlled state |
| Capture → read PPM → repeat capture | 0.03392 | 384×256 P6 image, 294927 bytes; repeated checksum `e2bf69ed3b77d540` |
| Deliberately remove delivery → diagnose → correct target and remove conveyor | 0.080836 | `invalid_value`, `FIXED_DELIVERY`; full state unchanged; tile query identifies fixed delivery; manifest, API text and diagnostic PNG present; removing conveyor (2,3) succeeds, delivery and tick unchanged |
| Verify shutdown | 0.005179 | Owned process exited and discovery registration removed |
| Probe old rule → edit scratch rule → incremental build → boundary probes | 0.876334 | Original binary accepts 61 and 60 (tick 121); advance cap changed from 36000 to 60 in source and metadata; 61 rejected with new message, 60 accepted, final tick 60 |

The last phase edits only the scratch `src/game.rs` validation limit and its
message/metadata. It is a deliberately small rule change suitable for the
construction skeleton. It is not a proposed game rule and is deleted with the
scratch tree. Construction legality and existing game code are untouched in the
working checkout. It uses a fresh process after the build; no hot reload is
claimed. The rerun first executes 61 and 60 ticks on the original binary (both accepted;
121 final ticks), then edits/rebuilds and executes the same probes (61 rejected,
60 accepted; 60 final ticks). That before-and-after probe is included in the
rule-change phase timer. The first sample did not include the baseline probe.

The rerun also extends diagnosis to recovery: after diagnosing the rejected
delivery removal, it corrects the target to the conveyor at (2,3), removes it,
and queries both tiles. It asserts the conveyor is absent, the delivery is
unchanged, precisely the conveyor disappeared from the structure list, and
tick 60 is preserved. The first sample stopped after diagnosis. The rerun was
requested by independent agent review to complete these verification boundaries;
it was not a retry of a runtime failure.

Every measured phase in each sample passed on its first attempt. There was one intentional
runtime rejection in the failure exercise and one intentional rejection in the
changed-rule probe, zero unexpected failed exercise attempts and zero human
interventions. The expanded procedure was rerun once; there were no failed-attempt retries or
human-relayed observations. The agent
prepared the harness by reading `games/factory/README.md`, `src/main.rs`,
`src/game.rs`, `scripts/test-control.py`, the factory slice, requirements,
workflow, CLI documentation and Titan workflow skill. This source/harness
reading is a setup cost excluded from the phase timers.

The README supplied the sequence encoding, mutation opt-in, project selection,
recording bounds and the warning that sequence process success does not mean
every operation succeeded. No blocking documentation gap was encountered for
this narrow exercise. The diagnostic bundle's exact shape was verified in the
run; the report keeps its key inventory and artifact existence, excluding raw
local paths or discovery credentials. Scratch captures, diagnostic files and
source edits are deleted after assertions; the report retains checksums and
semantic evidence, not reusable image files. PPM format/size and repeated pixels
are checked programmatically; visual appearance was not manually inspected.

Native GPU and actual browser/WASM play are documented in the game README and
have prior game evidence, but were not rerun in this measurement. They remain
unmeasured here. This run covers software capture and ordered construction
sequence replay, not interactive snapshot/replay controls. Transport,
extraction, processing, delivery progress and victory are unavailable at this
revision and therefore have no passing measurement. Later #93 verification can
repeat this procedure and add those paths with a new revision and explicit
boundaries; existing construction/replay results cannot substitute for them.
