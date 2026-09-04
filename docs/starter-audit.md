# Starter boundary audit

The RPG remains the accepted regression game. The starter is a separate Cargo
package under `starters/minimal`, with a replaceable `src/game.rs`. No engine
feature or generator is required by this audit.

| Existing piece | Reusable responsibility | Game assumptions to remove |
| --- | --- | --- |
| `examples/procedural_rpg.rs` | Bounded server lifecycle, authenticated discovery, safe-point queue drain, mutation policy, diagnostic wrapper | RPG construction, reference replay, filenames, quest and position state |
| `examples/play_rpg.rs` | winit lifecycle, key aliases, focus clearing, fixed-time accumulator, bounded presentation | RPG builder, title, status, reference replay, movement sampler |
| `examples/support/gpu_surface.rs` | Surface configuration and rendering public `RenderFrame` / `ImageAssets` | Device label only |
| `crates/titan-browser/src/lib.rs` | Synchronous protocol boundary, schema/target correlation, control opt-in, PNG encoding | RPG constructor, inspector registration, render hook, reference tests |
| `crates/titan-browser/src/player.rs` | Canvas lifecycle and bounded 60 Hz accumulation | RPG input, status and reference replay |
| `scripts/build-browser.py` | Match wasm-bindgen CLI to resolved library version | Root paths, package and WASM stem, output directories |
| `web/inspector`, `web/play` | Same-window/origin bridge, keyboard lifecycle | RPG copy, commands, entity fields and replay route |

The RPG's interactive input helper emits tile movement pulses every six ticks.
That is a game rule, not generic keyboard sampling. Starter input uses its own
continuous movement and exact future-frame snapshots. Commands, explicitly
validated fields, scene construction and diagnostic state remain game-owned.

The small host adapters live in the copied package and call public Titan APIs.
This keeps their customization visible without creating an unproven host
framework. The duplicate surface adapter is a candidate for later consolidation;
it is not a dependency on RPG support files.

A standalone `[workspace]` and explicit package metadata prevent accidental
workspace inheritance. Dependency paths must be configured after copying. The
starter build script resolves the target directory and WASM package locally.
`scripts/test-starter.py` verifies the native workflow from a temporary directory
outside the checkout so in-tree path assumptions cannot pass unnoticed.

Security and execution constraints are unchanged: authenticated native discovery,
explicit browser control opt-in, bounded diagnostics, future input validation,
and simulation access only at safe points. A transport timeout does not cancel
an already executing system.
