# Titan test selection

The shared CI change action selects tests for `pull_request` and `merge_group`
using the three-dot merge-base diff. Rename detection is disabled so both old
and new paths are considered; deletions and integration-test/benchmark/data
changes within a package count too. The `titan-tests` check always reports a
result, even when no Titan tests apply. There is no workflow-level path filter.

## Conservative policy

- A known Titan package change selects that package plus transitive downstream
  workspace consumers. Only Titan consumers are tested, but traversal includes
  non-Titan workspace intermediaries. Packages in `demos/` are identified from
  Cargo metadata, not inferred from their directory names.
- The graph uses `cargo metadata --no-deps` and **all declared workspace path
  dependencies**: normal, build, dev, optional, renamed, and target-specific.
  It deliberately over-approximates feature activation rather than relying on
  a default-feature resolve graph. This covers default, all-features, explicit
  render-unification and headless recipes, and may test consumers whose relevant
  dependency is disabled on Linux. No registry fetch or compilation is needed.
- Any manifest change (including package additions/removals, dependency and
  feature edits), root manifest/lockfile, shared configuration, toolchain, CI,
  upstream, or unknown path change runs full Titan coverage. Unknown Titan
  directories, incomplete/ambiguous metadata, path dependencies outside the
  known workspace, invalid refs, missing merge bases and classifier errors also
  select full coverage. Root docs and ordinary `docs/` changes have no applicable
  tests; generated `docs/cargo_features.md` still runs full coverage, following
  the shared path classifier.
- Main/release pushes, schedules and manual runs always select all Titan crates.
  Lint/doc package selection is unchanged. A failed classification job or missing
  package output falls back to full coverage; malformed/stale package lists are
  also rejected by the test command runner in favor of all packages.

## Recipes and auditing

Default-feature tests retain `--lib --bins --tests` (including integration tests)
with `bevy_ecs/track_location,bevy_remote/bevy_render`, plus Linux `--benches`
smoke coverage with render unification. Full upstream runs retain the workspace
CI default-feature tests instead. Extra tests run `--all-features` for each
selected package, adding `bevy_remote/bevy_render` for `titan_remote` and
`titan_mcp`. `titan_doom` and `titan_puzzle` also retain headless
`--no-default-features` tests and `--all-targets` clippy. New Titan crates get
all-features coverage automatically.

Classification summaries report the package list (`*` means all, `none` means
inapplicable) and the selection/fallback reason. Test job summaries expand the
list and record the recipes being executed. Default coverage on full runs is
recorded by the CI workspace test job.

The classifier tests use disposable Git repositories and synthetic Cargo
metadata; command tests use stubbed Cargo plus real jq. They exercise LF/CRLF
boundaries without compiling Rust:

```sh
node --test .github/scripts/titan-packages-changed.test.cjs
bash .github/scripts/test-ci-paths-changed.sh
bash .github/scripts/test-titan-scoped-ci.sh
```

## Performance verification

The issue's all-Titan baselines were 17m32s for PR #100 (4m48s default,
10m59s extra) and 15m28s for PR #101 (3m49s default, 8m42s extra).
A representative leaf-change CI measurement will be recorded here before
review readiness.

Selection reduces unrelated test targets, not necessarily all their dependency
compilation. Restored workspace artifacts may need rebuilding when package or
feature unification differs from the cached invocation, especially when moving
between default, all-features and headless recipes. Cache restore time and
runner variability remain fixed costs; timings should report the restored
cache and per-step durations, not imply a guaranteed speedup or timing target.
