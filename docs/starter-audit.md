# Starter boundary audit

The RPG remains the accepted regression game. The starter is a separate Cargo
package under `starters/minimal`, with a replaceable `src/game.rs`.
Milestone 2 needed no engine changes. The subsequent
[host setup audit](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/host-setup-audit.md) extracts demonstrated host responsibilities
without introducing a generator or game framework.

| Current piece | Shared responsibility | Game-owned responsibility |
| --- | --- | --- |
| `titan_remote::Server`, queue, `DiagnosticInspector` | Authenticated discovery, safe-point dispatch, bounded diagnostic policy/history/writing | Controlled runner lifetime, construction, replay, paths and diagnostic state |
| `titan::input::update_button_alias` | Combining held physical aliases | Action mapping, focus clearing, movement sampler |
| `titan_render_wgpu::SurfaceRenderer` | Surface configuration and presentation of `RenderFrame` / `ImageAssets` | Window/canvas creation, extraction, title, size and cadence |
| `titan::inspection::BrowserSession` | Synchronous protocol boundary, schema/target correlation and control opt-in | Constructor, inspector registration and exported JS wrapper |
| `titan_diagnostics::png_capture` / `write_png` | Exact PNG encoding | Render hook and capture destination policy |
| Native `play` / browser player adapters | Explicit composition of public APIs | Lifecycle, accumulator, status, restart and reference replay |
| `scripts/titan_build.py` | Matching wasm-bindgen tooling and Cargo-reported binary bundling | Package/binding/application names and entrypoints |
| Copied `web/inspector`, `web/play` | Standalone same-origin bridge and keyboard lifecycle, with tests | UI, commands, entity fields and replay route |

The RPG's interactive input helper emits tile movement pulses every six ticks.
That is a game rule, not generic keyboard sampling. Starter input uses its own
continuous movement and exact future-frame snapshots. Commands, explicitly
validated fields, scene construction and diagnostic state remain game-owned.

The small host adapters live in the copied package and call public Titan APIs.
Surface management, browser request policy, PNG encoding and packaging are now
shared, while construction, mappings, capture selection and lifecycle composition
remain visible in the game. No adapter depends on RPG support files.

A standalone `[workspace]` and explicit package metadata prevent accidental
workspace inheritance. Dependency paths must be configured after copying. The
starter build script resolves the target directory and WASM package locally.
`scripts/test-starter.py` verifies the native workflow from a temporary directory
outside the checkout so in-tree path assumptions cannot pass unnoticed.

Security and execution constraints are unchanged: authenticated native discovery,
explicit browser control opt-in, bounded diagnostics, future input validation,
and simulation access only at safe points. A transport timeout does not cancel
an already executing system.
