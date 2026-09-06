# Finished factory: bounded larger-fixture measurement

This is fresh native evidence for [#93](https://github.com/titan-engine/titan/issues/93)
at full engine/game revision `e4800939606889669e8a9b04650cda4bce6df37d`.
It does not reuse the historical construction-only measurements at
`0468ffe00b2cb109acc33591dc382839196ce7fe`. It follows the phase boundaries and
reporting distinctions in [the shared procedure](../../../agent-iteration.md).

Run from a checkout containing this report and the measured git object:

```sh
python3 docs/evidence/factory-verification/scaling/measure.py
```

The script archives the pinned revision, adds [the disposable probe](probe.rs)
as an additional Rust binary, builds with `--locked` in an empty private target,
then deletes that archive and target. It does not edit game rules, inject items,
or write gameplay source in the working checkout. Existing global Cargo registry
and toolchain caches remain available. The accepted build/runtime deadlines are
hang bounds, not speed requirements. Measurements overwrite `results.json` and
the generated fixture JSON files; preserve the committed report when rerunning.

The probe uses public `build_game`, `player_command`, `status`, `render_image`,
and `image_checksum` APIs. Each fixture contains the exact ordinary placement
operations plus warmup advance. Native windows, discovery servers and browsers
are not launched by this experiment. GPU/browser evidence belongs to the separate
playtest; software rendering here is not a usability or GPU verification claim.

## Workload and semantic checks

All runs use the fixed 12×8 world and its single deposit. The comparison dimensions
are occupied structures, geometrically compatible directed connections, item
occupancy, machine kinds, active production and saturated transport. The fixtures
are deliberately bounded by the existing grid:

- `reference_active`: the documented ten-structure delivery line; warmup 600 ticks.
- `long_active`: a 49-structure world including delivery, with a 48-tile serpentine
  line from the deposit through a processor at (5,4), ending disconnected at (0,7).
  The processor's output has a 42-hop path to the end. Warmup 600 ticks.
- `dense_active`: the same active path with every other buildable tile occupied by
  an idle north-facing processor, for all 96 tiles occupied. Warmup 600 ticks.
- `dense_stalled`: the identical dense construction warmed for 12,000 ticks, enough
  for the extractor, ore queue, processor and plate route to fill and stall.

Each workload runs three fresh constructions followed by 600 individually issued
one-tick advances. At every measured tick the probe independently counts all
nonempty input, in-process and output slots and checks that resident items plus
deliveries and both explicit discard counters equal extracted plus seeded items.
It does not rely on the game's `conserved` flag. It also checks no diagnostic and
exact tick advancement. The full serialized public state at every measured tick
is compared directly with the first repeat, including order, ports, slots, progress,
camera, selection and counters; equality is not inferred from a checksum alone.
Warmup is checked at its operation boundary, not at each warmup tick.

The reference finishes the measurement at tick 1200 with nine deliveries. Both
long active layouts have nine plate outputs, no deliveries, and a plate at the
far end, proving processing plus transport across the longer route. For the
saturated layout, the probe requires structures (including every slot, work counter
and transport reason), extraction, deliveries and discard counters to remain
unchanged between the start and end of the measured interval, plus exact full-state
equality across repeats. These assertions count as verified only if that entire
probe finishes successfully; a timeout supplies no passing saturation evidence. These are independent expected outcomes, not
merely tests that commands returned success. Final public state and independently
computed workload counts accompany completed samples in `results.json`. Duplicate
initial/final snapshots are checked for exact equality and refer to repeat 0.

## Timing boundaries and limits

Each advance sample sums monotonic elapsed time around 600 public one-tick command
calls. This includes command parsing, command result generation and operation
recording, but excludes the subsequent status serialization and semantic checks.
It is not a bare engine scheduler measurement. Interleaved inspection changes
cache conditions; the three runs are not independent machine/environment samples.
Setup timing includes ordinary construction, warmup and checks after each setup
operation. Separate inspection timing covers 20 state serializations, JSON parses
and equality checks; separate software capture timing covers ten 384×256 renders
and image checksum computations. Those captures are also checked for identical
pixels, and state is unchanged after all read-only inspection/capture work.

The report records build duration and each entire probe subprocess duration too.
The latter includes setup, correctness checks and comparisons outside the timed
advance calls. Reading documentation, preparing the harness, agent reasoning and
scratch extraction/deletion are excluded from these command/phase measurements;
no end-to-end authoring latency or player frame-rate claim is made. The unfamiliar
variation exercise is reported separately.

The host is shared with other factory/adventure agents and their builds; CPU,
filesystem and global registry contention are uncontrolled. No target directory
is shared by this probe. Debug-profile results on this host cannot establish
portable performance, percentiles, asymptotic behavior, a supported larger world,
or release-profile capacity. More structures can cost more even when their
processors are starved; the dense comparison does not increase production rate
because the approved world still has one extractor. There is no machine-dependent
CI threshold and no optimization or new gameplay system in this experiment.

## Attempts and tooling observations

The first attempt completed the three active fixtures, then exceeded the default
60-second aggregate runtime deadline on the dense saturated fixture. No semantic
assertion failure was observed; the timeout does not prove the still-running
workload correct. The initial runner wrote its report only at the end, so those
partial sample timings/states were lost. [The sanitized attempt record](attempts.json)
preserves that failure and correction. The final runner saves completed fixture
results incrementally, retains caught failure details, and explicitly gives each
three-repeat subprocess a 300-second hang bound. All fixtures and gameplay
assertions remain the same; the full rerun uses another empty private target.
No performance requirement was relaxed because none was defined. The accepted
bound covers construction, long warmup and extensive correctness serialization,
not only the separately measured 600 command calls.

The evaluator was independent of factory implementation but read the public game
README, factory specification, shared baseline procedure and probe-relevant API
signatures. The command API sufficed to build both workloads without private ECS
access. One initial source search guessed a nonexistent `model.rs`; the README's
source map and `game.rs` public exports resolved that immediately. These are
scaling-harness observations, not the separate unfamiliar authoring experiment.

The second attempt retained all active results but also exceeded its 180-second
aggregate bound on the saturated fixture, with no observed assertion failure.
The final attempt uses the same fixture and assertions under a 300-second
aggregate deadline and `--resume`, rebuilding the pinned source/probe in another
empty private target and retaining the already completed active measurements:

```sh
python3 docs/evidence/factory-verification/scaling/measure.py --resume
```

Resume reads `results.json`, requires the same full revision, and skips workloads
already recorded there. Use the ordinary command without `--resume` for a full
independent reproduction. The final report records the previous attempt's build
duration and failures separately from the resumed build. A repeat's 12,000-tick
warmup is intentionally substantial relative to the 600 measured ticks; aggregate
timeouts are documented harness failures, not semantic evidence of a jam bug.

## Observed results

All four workloads completed three repeats with every programmed semantic check passing. The final dense stalled subprocess completed in 273.998 seconds within its 300-second bound. There were two prior aggregate timeout failures and no observed semantic assertion failure.

Measured host: macOS 27.0 arm64, Apple M5 Pro (18 logical CPUs), Rust/Cargo 1.98.1, Python 3.9.6. The active-fixture build took 13.615 seconds; the later resumed stalled-fixture build took 12.714 seconds. Both used private empty targets.

| Workload | Structures / connections | Items start → end | 600 command calls, three samples (s) | 20 state reads, three samples (s) | 10 software captures, three samples (s) |
| --- | --- | --- | --- | --- | --- |
| reference_active | 10 / 9 | 5 → 5 | 0.353, 0.349, 0.369 | 0.023, 0.023, 0.027 | 0.082, 0.077, 0.091 |
| long_active | 49 / 47 | 10 → 15 | 2.322, 2.229, 2.207 | 0.119, 0.120, 0.120 | 0.105, 0.104, 0.104 |
| dense_active | 96 / 81 | 10 → 15 | 5.837, 5.847, 5.895 | 0.255, 0.273, 0.275 | 0.149, 0.161, 0.152 |
| dense_stalled | 96 / 81 | 50 → 50 | 7.807, 7.586, 7.583 | 0.298, 0.284, 0.291 | 0.171, 0.164, 0.165 |

The saturated run retained 50 resident items and zero deliveries from tick 12000 through 12600, with extraction and every machine/transport structure unchanged. Its 48 processors comprise one blocked production machine and 47 starved fillers. Active dense construction has the same nine output plates at tick 1200 as the unfilled long route. These results verify the bounded fixture; they do not establish capacity beyond this fixed grid.

All owned probe processes exited normally on the final attempt, and the scratch archive/build target was removed. There was no discovery registration or GUI reservation to release. Python syntax compilation and Rust formatting checks passed for the retained harness.
