# Control foundation verification

The control foundation was exercised on 2026-09-06 on macOS / Apple M5 Pro.
This is evidence for issue #81; jumping and puzzle progression are absent.

![Native GPU initial room](evidence/initial-native.png)

The capture shows the fixed elevated camera, narrow cyan Jumper with triangle,
broad orange Strong with square, active ground outline and active-name HUD.
No foreground wall hides the starts. This is an actual native Metal capture,
not a software reference or an image generated separately from the game.

## Reproducible checks

- Package Rust tests cover axial/diagonal/opposing movement, bounds, inactive
  stationarity, input switching and repeat edges, restart precedence, recording
  limits and transactional rejection, physical aliases and focus/resume handling.
- `scripts/test-control.py` drives the authenticated native server through the
  Titan CLI. `scripts/test-browser.mjs` runs a fresh native trace and compares
  every complete state against actual WASM executing the same 11-tick fixture.
  The final positions are Jumper `(1500,6500)`, Strong `(3500,6320)`, with Jumper
  active. Recording playback produces the same consumed input and positions.
- `scripts/test-player.py` presented native Metal GPU frames, observed an OS
  resize to 800 × 500 and zero-size suspension/recovery, and verified captures,
  read-only capture permission, session identities and interactive replay.
  Initial/repeated/reset/read-only captures share checksum `57f8d01ae2b745a4`;
  selecting Strong changes it to `45ee76242fa5818e`. These are local consistency
  checks, not a portable GPU hash contract.
- The actual browser `/play/test.html` passed separately with `backend=webgpu`
  (BrowserWebGpu) and `backend=webgl2` (Chromium ANGLE Metal, Apple M5 Pro).
  Both exercised keyboard sampling, switch suppression, 17-tick interactive
  replay, pause/resume, restart, owned asynchronous capture, capture cancellation,
  zero-size recovery, and 960 × 540 / 1280 × 720 presentation. The browser route
  ends at Jumper `(1980,6500)` and Strong `(3500,6020)`, with Strong active.
  The page displays assertions and provides identity-bearing capture evidence.
- Root workspace formatting, tests, Clippy and WASM core checks passed, together
  with the native and actual-WASM RPG control, replay and asset loops. Existing
  RPG references and the committed README preview were not changed.

Run commands and inspection examples are in the [game guide](README.md);
[quality gates](../../docs/implementation-plan.md) include this standalone package
in all three PR jobs. Native captures/JSON and bounded failure diagnostics are
retained under the repository's ignored `target/` directory; public review
comments identify the exact authored SHA and current CI evidence.

## Limits

Both character silhouettes and active selection have been visually inspected.
The renderer's optional fixed-aspect path preserves the existing default, while
unit tests check viewport fitting and invalid ratios. GPU image equality is
asserted only within one backend/session; no exact cross-GPU pixel claim is made.
Browser keyboard tests drive the real WASM player API and browser event adapter;
this is reproducible runtime evidence, not an extended human usability study.
The release-gating choice remains the documented initial prototype policy.
No other operating system or hardware performance claim is established here.
