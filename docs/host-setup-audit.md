# Host setup consolidation

The baseline is accepted milestone 2, commit
`5d63e12aabab1f6d5aa1ea3e9ab092a2f2b981f6` (`v0.2.0`). This work reduces
copied infrastructure while keeping RPG, minimal starter and arena independent.
It does not introduce a game trait, generic application runner or new crate.

## Boundaries

| Responsibility | Decision | Game-owned part |
| --- | --- | --- |
| GPU adapter/device, surface configuration, resize, acquire/present | Public `titan_render_wgpu::SurfaceRenderer` consumes `RenderFrame` and `ImageAssets` | Window/canvas creation, extraction, dimensions, title, render cadence |
| Native keyboard aliases | Public `titan::input::update_button_alias` combines physical button states | Key/action mapping, Escape/R behavior, input sampling and focus reset |
| Native lifecycle | Retain small explicit winit runners | Startup/replay, restart, exit rules, timing, presentation count |
| Browser request JSON and read-only policy | `titan::inspection::BrowserSession` | Game constructor, inspector registration and exported JS wrapper |
| Browser capture encoding | `titan_diagnostics::png_capture` / `write_png` | Rendering hook; native output path/format policy |
| Native controlled loop | Reuse existing authenticated `Server`, safe-point queue and `DiagnosticInspector` | CLI defaults, run bound, signal handler and diagnostic state |
| Diagnostics | Existing bounded history and writer already shared | Positions, health/outcome and other useful game evidence |
| Browser build and macOS bundling | Shared tooling resolved from Cargo's Titan dependency | Package/target, output names, application identity and entrypoint wrappers |
| Browser pages and message bridge | Retain copied editable web files | UI, same-origin boundary, controls and presentation |

The duplicated lifecycle is not sufficient evidence for an application framework.
The RPG's six-tick movement pulses, exact replay and lack of R restart differ
from the standalone games. Native Duration and browser floating-point clocks
also have different representations; changing them is unnecessary for this
objective. Focus loss still clears both held keys and game input. Web bridges
remain standalone scripts covered by their existing tests; Rust consolidation
must not create an import from a copied game to RPG web assets.

Native diagnostics already delegate request history, policy and bounded bundle
writing to public APIs. Their remaining closures select game state and report
capture failure; removing those lines would mostly hide useful customization.
Browser capture remains a software reference, and the playable browser instance
remains separate from the paused inspection instance. Transport timeout still
does not cancel an executing system.

## Measurement and verification

Each standalone game's host Rust files (`src/lib.rs`, `src/main.rs`,
`src/browser.rs`, removed `src/surface.rs`, `src/bin/play.rs`) plus its browser
and bundle entrypoints and new `scripts/titan_tools.py` loader fell from **1,066
to 739 physical lines**: 327 fewer copied lines (31%). Counts include tests,
comments and blank lines in those files; exclude `game.rs`, web assets, docs,
and shared engine implementation. Reproduce by counting `splitlines()` for
those paths at the baseline above and the current source (missing files count
as zero). This measures setup/maintenance surface, not authoring productivity.

Three 144-line surface modules became one 151-line public module. Each player
now selects its own extracted frame/assets explicitly. Browser adapter files
fell from 313/358/358 lines (RPG/starter/arena) to 211/266/266, including the
additional explicit render selection in the standalone files. Policy and PNG
behavior have dedicated shared tests. Shared APIs also add documentation and
tests outside the copied games, so these figures are not whole-repository size
claims. [Tooling measurements](host-tooling.md) separately count the shared
implementation: 292 to 209 lines across the build entrypoints/helpers.

The [fresh independent verifier](host-workflow-verification.md) passed the
external-copy native/WASM workflow without RPG imports or engine-internal
inspection. Its warm acceptance run took 9.00 seconds; concurrent work and cache
reuse prevent a before/after speed claim. No build speedup is claimed.

All documented root quality gates, native and actual-WASM control loops,
starter external-copy checks, arena tests/Clippy/WASM/bridge checks, portable
build-tool tests and copied/relocated macOS bundles passed locally. The complete
command results are in [host-checks.json](host-checks.json); timings are one warm
integration run, not a benchmark. Independent code review found no actionable
regressions in policy, PNG fidelity, tooling resolution or package selection.

Hardware evidence on macOS:

- Both renderer offscreen tests passed; RPG replay passed again with
  `TITAN_GPU_TOLERANCE=0`.
- Arena GPU readback passed initial, active and loss comparisons on unorm/sRGB.
- RPG reference player, starter and arena each presented two native GPU frames
  and exited normally.
- Rebuilt browser RPG canvas completed frame11 with all three shards and active
  shrine. Arena's bounded `/test/` fixture won at host frame1200 with health2,
  then restarted and lost at host frame2400/game elapsed310 with health0. Both
  canvases were visually inspected. Starter Play/Pause/Restart rendered correctly
  and preserved paused frame227.

Software references remain RPG `190a92085def5677`, arena initial
`1e5d05f547d53435`, arena winning replay `be61b1c710b101b6`. Game rules and art
were unchanged. Hardware/browser inspection is integration evidence, not a
replacement for those exact software references. Existing CI jobs still cover
all targets, with an additional portable build-tool policy step.

