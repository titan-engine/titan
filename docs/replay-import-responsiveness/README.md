# Arena replay import responsiveness

[Issue #34](https://github.com/titan-engine/titan/issues/34) measures the existing
synchronous validation-before-install path at both import limits. This is an
investigation snapshot, not a performance guarantee or a new validation design.

## Reproduce

Use an otherwise idle machine and build optimized native and browser artifacts:

```sh
cargo build --release --manifest-path games/arena/Cargo.toml --bin titan-game --bin play --bin replay
cargo build --release -p titan-cli
python3 games/arena/scripts/build-browser.py
python3 docs/replay-import-responsiveness/native.py > native-output.json
python3 -m http.server 8080
```

Verify `git status --short` is empty, then open
`http://localhost:8080/games/arena/web/test/?revision=<full-git-revision>&working_tree_dirty=false`,
select **Replay import responsiveness**, and use **Download import evidence**.
The fixture requires both provenance parameters and writes them into its report;
use `working_tree_dirty=true` instead if measuring local changes. The native
probe discovers the same Git values itself and uses the real GPU player. Each
probe uses seven samples and asserts that a recording rejected only by the final
snapshot/pixel check leaves the current session unchanged.

The 3,600-tick fixture is an idle-input recording produced through the actual
arena session. Its compact JSON is well below the byte limit, so the exact 2 MiB
case adds leading JSON whitespace. This isolates maximum accepted parsing size
without changing the parsed recording or file format. Native control uses the
compact recording because the CLI's separate argument-file limit is 1 MiB; the
native `replay` verifier measures the exact 2 MiB file.

## Results

Recorded on 2026-09-05 on an otherwise idle Apple M5 Pro with 64 GiB RAM,
macOS 27.0 (26A5425), arm64. Native used Rust 1.98.1 optimized binaries. Browser
used the optimized WASM build in Chromium 152 through the Codex in-app browser.
The evidence revision and exact per-sample values are recorded in
[native-output.json](native-output.json) and
[browser-output.json](browser-output.json).

Values below are median milliseconds with the observed seven-sample range in
parentheses. Native file time includes process launch; native control time
includes a fresh CLI process and the player's approximately 16 ms event-loop
wake interval. Browser timer and animation-frame measurements start immediately
before the synchronous import call.

| Case | Native file | Native control load | Concurrent native status | Browser call | Browser timer | Browser animation frame |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Short valid, 8 ticks / 2,581 bytes | 3.974 (3.688–4.372) | 16.280 (8.524–16.656) | 13.176 (5.353–13.413) | 0.400 (0.300–0.600) | 0.500 (0.400–0.700) | 8.200 (0.500–8.400) |
| Maximum valid, 3,600 ticks / about 150 KiB | 10.346 (9.633–10.552) | 16.643 (15.944–21.578) | 13.389 (12.669–17.784) | 8.400 (7.500–10.100) | 8.400 (7.700–10.200) | 8.400 (7.700–10.300) |
| Maximum valid, 3,600 ticks / exactly 2 MiB | 10.970 (10.689–11.243) | n/a | n/a | 10.000 (9.800–10.600) | 10.000 (9.800–10.900) | 10.100 (9.800–11.700) |
| Rejected at final verification, 3,600 ticks | 9.349 (8.896–9.576) | 14.753 (14.216–22.813) | 10.910 (4.820–19.491) | 6.900 (6.900–7.000) | 6.900 (6.900–7.100) | 7.000 (6.900–7.400) |

All 21 native control samples still had the load CLI process running when the
status probe began, and no probe timed out. Process overlap does not prove the
load handler had already started, so the native values bound observed client
responsiveness rather than isolating handler time. Direct native verifier and
browser-call measurements isolate the scaling more clearly. The browser's
zero-delay timer moved by essentially the synchronous call duration. No measured
browser animation-frame delivery exceeded 11.7 ms, below one 60 Hz frame interval;
an import can still consume more than one 120 Hz frame budget.

Every final-mismatch sample replayed all 3,600 ticks and was rejected. The live
session's frame/revision, exported save, replay status and software-pixel checksum
matched before and after every rejection on both hosts.

## Assessment

Incremental validation is not justified by this evidence at the current 3,600
tick / 2 MiB limits. The reference machine showed no user-visible multi-frame
pause, timeout or loss of native control responsiveness, while an incremental
design would add pending-state, cancellation and session-generation complexity
to a currently transactional path. Keep synchronous validation for now.

This is one fast reference machine, one release build and the current arena
systems, not a device matrix or latency guarantee. Revisit the decision with a
separate approved issue if slower target hardware, materially more expensive
per-tick game logic, raised import limits, or field reports produce repeated
multi-frame stalls. A follow-up should define a responsiveness target and sample
the affected targets before changing the architecture.

Any future design must keep the existing transactional boundary: fully validate
the complete recording before installation, cancel if the browser file-read
session changes, and compare the exact final snapshot and software pixels. It
must also define how work is canceled when a live session changes while batches
are pending. Seeking's 120-tick batching cannot simply be reused because seeking
operates on an already validated and installed recording.
