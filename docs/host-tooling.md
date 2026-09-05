# Shared build tooling

`scripts/titan_build.py` is a public Python 3 helper shipped with the Titan
source dependency. It requires Cargo and rustup; browser builds also use Node
for the separately invoked WASM tests. Games retain small entrypoints and their
own browser pages, binding names, application names and bundle IDs.

- `cargo_metadata(root)` returns resolved Cargo metadata and respects
  `CARGO_TARGET_DIR`.
- `browser(root, metadata, package_name=..., out_name=...)` builds the named
  package's single cdylib for release WASM, resolves the matching wasm-bindgen
  CLI, and writes web bindings to `root/web/inspector/pkg` and Node bindings to
  Cargo's `target_directory/titan/browser-node`.
- `macos_app(root, metadata, argv=None)` parses the documented `--bin`, `--name`,
  `--bundle-id` and `--release` flags and packages Cargo's reported binary path.
  It prints the absolute unsigned development `.app` path. Signing,
  notarization and distribution remain outside this helper.

Each copied game's `scripts/titan_tools.py` locates the helper using the resolved
`titan` package's manifest path. Configure normal Cargo dependency paths after
copying, or use a Git dependency on a revision containing this helper. No RPG
source or fixed checkout location is needed. More than one resolved package
with a required name is rejected explicitly rather than choosing an arbitrary
version. A game with another web layout can call the bindings tool itself;
this is a narrow convention, not a project generator or general build CLI.

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
