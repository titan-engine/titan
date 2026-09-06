# Shared host and build tooling

## Host boundaries

Games compose public APIs explicitly. Titan does not own a game's window or
event loop or require a generic application runner.

| Shared API/tooling | Game-owned responsibility |
| --- | --- |
| `titan_render_wgpu::SurfaceRenderer` / `SurfaceRenderer3d`: device acquisition, surface configuration, resize and presentation | Window/canvas creation, extraction, dimensions, title and cadence; see [rendering](rendering.md#extraction-and-rendering) and [3D presentation](rendering.md#drawing-and-presentation) |
| `titan::input::update_button_alias`: combining held physical aliases | Key/action mapping, input sampling, focus reset, Escape/restart behavior |
| `titan::inspection::BrowserSession`: synchronous request JSON and control opt-in | Constructor, inspector registration and exported JS wrapper; see [browser inspection](browser.md) |
| `titan_diagnostics::png_capture` / `write_png`: exact PNG encoding | Render hook and native capture path/format policy |
| `titan_remote::Server`, safe-point queue and `DiagnosticInspector`: authenticated control and bounded diagnostics | Controlled runner lifetime, CLI defaults, signal handling, replay and useful diagnostic state; see [inspection](inspection.md) |
| `scripts/titan_build.py`: browser builds and macOS bundles resolved from the Cargo dependency | Package/binding/output names, application identity and entrypoint wrappers |
| Copied browser pages and message bridge | Editable UI, same-origin boundary, controls and presentation; no imports from RPG web assets |

Keep native lifecycle and timing explicit: startup, replay, restart, exit rules
and presentation count differ between games. Focus loss clears both held keys
and game input. Diagnostics closures select useful game state and report capture
failures; a transport timeout does not cancel an executing system.

The RPG/starter/arena synchronous inspection adapters use software captures and
keep the paused browser inspection instance separate from the playable instance.
Live-player inspection and owned asynchronous GPU capture are distinct contracts:
see [live-player inspection](live-player.md) and
[asynchronous capture](inspection.md#asynchronous-capture-contract). Consult each
game README for its actual player, restart, input and capture semantics.

## Build tooling

`scripts/titan_build.py` is a public Python 3 helper shipped with the Titan
source dependency. It requires Cargo and rustup; browser builds also use Node
for the separately invoked WASM tests. Games retain small entrypoints and their
own browser pages, binding names, application names and bundle IDs.

- `cargo_metadata(root)` returns resolved Cargo metadata and respects
  `CARGO_TARGET_DIR`. Metadata, browser builds and native bundles use `--locked`.
- `browser(root, metadata, package_name=..., out_name=...)` builds the named
  package's single cdylib for release WASM, resolves the matching wasm-bindgen
  CLI, and writes web bindings to `root/web/inspector/pkg` and Node bindings to
  Cargo's `target_directory/titan/browser-node`.
- `macos_app(root, metadata, argv=None)` parses the documented `--bin` or `--example`, `--name`,
  `--bundle-id` and `--release` flags and packages Cargo's reported binary path.
  It prints the absolute unsigned development `.app` path. Signing,
  notarization and distribution remain outside this helper.

Both helpers accept `assets_source=Path(...)`. If omitted, an existing
`root/assets` is used; projects without one need no resources. An explicit missing
source fails. Regular files are staged into `web/assets` or
`Contents/Resources/assets`; successful builds replace that generated directory.
Symlinks and source/output overlap are rejected. Root RPG wrappers explicitly
require `assets/`; `scripts/build-rpg-app.py` selects the `play_rpg` example.
See [RPG asset iteration](assets.md) for resource lookup and replacement rules.

Each copied game's `scripts/titan_tools.py` locates the helper using the resolved
`titan` package's manifest path. Configure normal Cargo dependency paths after
copying, or use a Git dependency on a revision containing this helper. No RPG
source or fixed checkout location is needed. More than one resolved package
with a required name is rejected explicitly rather than choosing an arbitrary
version. A game with another web layout can call the bindings tool itself;
this is a narrow convention, not a project generator or general build CLI.

Verification requires an existing, current project `Cargo.lock` and fails with
Cargo's lockfile diagnostic when it is missing or stale. No helper retries with
unlocked resolution. After configuring a new copied project, explicitly run
`cargo generate-lockfile` in that project. For deliberate dependency changes,
use `cargo update -p PACKAGE` or regenerate the whole graph intentionally,
review and commit the resulting lockfile, then rerun locked verification. The
workspace, starter and games keep independent lockfiles.

Browser builds reuse a CLI under the game's Cargo target directory, the Titan
checkout's default target directory, or PATH only if its reported version
matches the resolved wasm-bindgen library. Otherwise the helper installs that
exact CLI version into the game's target directory. This cache lookup existed
in the copied game scripts already; extraction does not imply faster builds.

## Verification and measured setup change

```sh
python3 scripts/test-build-tools.py
python3 scripts/test-starter.py --browser
python3 scripts/test-macos-bundles.py  # macOS only
```

Portable policy tests cover stale CLI rejection, matching dependency-cache
reuse, ambiguous resolution, custom Cargo targets and library names, exact
reported native executable selection, bundle metadata and invalid app names.
The external-copy checks cover actual compilation and loader resolution;
macOS checks additionally rename/relocate both games' bundles and run their
embedded binaries. These checks run in CI.

Before extraction, the root browser script and the two games' browser/bundle
scripts totaled 292 physical lines. The same five entrypoints plus the shared
helper and two copied loaders total 209 lines, an 83-line reduction (28%).
Within each copied game, build setup drops from 129 to 38 lines (71%), counting
its loader. Counts include comments and blank lines, exclude tests/docs, and
compare the accepted milestone-2 source against this extraction. No compile or
iteration speed improvement is claimed. The same Cargo targets and release
profile are built; full dependency metadata for macOS bundling adds a small
resolution step in exchange for locating dependency-owned tooling.
