# Independent arena verification

A fresh verification agent checked the current arena on 2026-09-05 using
[the milestone brief](second-milestone.md), [arena README](../games/arena/README.md),
[exercise record](arena-exercise.md), implementation plan, and titan-workflow
skill. It reviewed the arena game, host adapters, manifest and test scripts.
It did not read the RPG implementation/history or engine implementation source.
No extra API explanation or undocumented engine access was needed. The default
arena Cargo target directory reused the builder's compiled dependency cache.

## Reproduced checks

These commands ran from `games/arena`, all with exit status 0:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo test --all-targets` | Five library tests and two native-player tests passed; the one GPU test was intentionally ignored in this invocation. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed without warnings. |
| `cargo check --lib --target wasm32-unknown-unknown` | Passed. |
| `cargo run --bin titan-game` | Initial state: frame0, player(80,65), elapsed0, health3, spawned0, Running; initial software capture written. |
| `python3 scripts/test-control.py` | Native discovery, inspected fields, input, stepping, valid edit, invalid/disabled edits, win, idle loss, restart, captures, diagnostic contents and discovery cleanup passed. |
| `python3 scripts/build-browser.py` | Built the browser and Node WASM packages successfully. |
| `node scripts/test-browser.mjs` | Actual compiled WASM policy, input, capture, fields, restart, survival and loss assertions passed. |
| `node --test web/inspector/bridge.test.mjs` | Four tests passed, including source/origin rejection and structured failure forwarding. |

The native script regenerated `games/arena/target/arena-evidence/initial.ppm`,
`won.ppm`, `lost.ppm` and `verified.json`. Initial checksum is
`1e5d05f547d53435`; the 1200-tick winning route produces `be61b1c710b101b6`
in both native and actual WASM. Rust assertions establish health2, five spawns,
Won, and exact idle loss at game tick310. The initial enemy is (124,105).

An additional transient Node assertion run loaded the generated
`target/titan/browser-node/titan_game.js` directly. It used the same request
envelopes as the checked-in browser script and independently verified:

- A frame0 injection, unknown movement action and axis-valued movement fail with
  `invalid_value`.
- Right at frame1 moves x80 to x81; stepping frame2 with no submitted snapshot
  leaves x81, confirming release semantics.
- x154, y17, y106 and fractional x edits fail, retaining player(81,65).
- A queued right input is cleared by restart; the next step leaves player(80,65).
- The documented route ends at (140,65) with the winning checksum; 100 additional
  ticks leave that capture unchanged.
- Restart followed by the same route, using the new host-frame offset, wins with
  the same checksum again. This checks reset RNG and game timing independently
  of resetting the host clock.

These supplemental assertions passed; they are review evidence rather than an
additional checked-in regression script.

## Review and diagnostic evidence

No blocking game correctness gap was found. Simulation uses integer state and
fixed ticks, deterministic spawn RNG, axis pursuit, seven-pixel contact overlap,
a global 60-tick damage cooldown, and terminal outcomes that stop simulation.
The final-tick health check precedes winning, so lethal contact loses. Restart
resets run state and input, deactivates the reusable enemy pool, restores the
player, and retains protocol time. Both graphical hosts call this same game
definition. The manifest and arena source/scripts contain no RPG imports or
assets. Coordinate metadata exposes validated x0..153 and y18..105; native field
edits require launch opt-in, while browser mutation operations require explicit
control opt-in.

The retained failed-attempt response and bundle agree on frame495 and
`invalid_value`. The 73,168-byte bundle records health0, Lost, six spawns,
player(98,18), enemy(104,18), and a final accepted right input for tick495.
Its history is bounded to 64 requests and 62 accepted inputs, with 433 dropped
entries explicitly recorded. Its capture metadata names the retained
160-by-112 `capture.png`, checksum `e93254900d6349ed`. This supports the exercise
record's overlap diagnosis; it is not an independent reproduction of the old
90/3 tuning. The newly run control script also generated fresh bounded failures
and checked their run-state/input contents.

## Scope and limitations

No public-source or documentation insufficiency blocked this verification.
The README provided sufficient commands and semantics. The verifier did not
repeat the historical tuning experiment or the external starter-copy exercise.
It did not run a graphical window, GPU readback, browser canvas interaction,
root workspace gates, or CI; the coordinating agent handles those separately.
Node actual-WASM and bridge tests establish runtime/control behavior, not browser
GPU presentation. User review of the playable result is still a separate
acceptance step; this report does not claim that approval.
