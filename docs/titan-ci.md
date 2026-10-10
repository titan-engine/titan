# Titan test selection

The shared CI change action selects tests for `pull_request` and `merge_group`
using the three-dot merge-base diff. Rename detection is disabled so both old
and new paths are considered; deletions and integration-test/benchmark/data
changes within a package count too. The `titan-tests` check always reports a
result, even when no Titan tests apply. There is no workflow-level path filter.

## Conservative policy

- A known Titan package change selects that package plus transitive downstream
  workspace consumers. If the closure contains a non-Titan consumer, it requests
  upstream/full workspace coverage instead of silently omitting that consumer.
  Packages in `demos/` are identified from Cargo metadata, not inferred from their
  directory names.
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
with ECS tracking and applicable remote render unification, plus Linux
`--benches` smoke coverage with applicable render unification. Feature arguments
are routed through direct dependencies accepted by Cargo: ECS-only leaves do not
request unrelated remote features, and demos use `bevy/track_location`. A
metadata-only feature planner follows default/forwarded features (including
optional and weak forwarding), conservatively unions target predicates, and
falls back to all Titan packages if an active dependency has no unconditional
runtime/dev CLI route. Build-only or platform-inapplicable direct routes cannot
silently substitute for runtime features under Cargo resolver 2/3.
Current Titan leaves and demos all have valid selective routes. Full upstream
runs retain the workspace
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
boundaries without compiling Rust. Feature-routing regressions also run real
`cargo tree --offline` in tiny registry-free workspaces (no builds):

```sh
node --test .github/scripts/titan-*.test.cjs
bash .github/scripts/test-ci-paths-changed.sh
bash .github/scripts/test-titan-scoped-ci.sh
```

## Performance verification

The issue's all-Titan baselines were 17m32s for PR #100 (4m48s default,
10m59s extra) and 15m28s for PR #101 (3m49s default, 8m42s extra).
A representative leaf probe in temporary [PR #105](https://github.com/titan-engine/titan/pull/105)
added one `titan_determinism` integration test and selected only that package.
Its [successful CI job](https://github.com/titan-engine/titan/actions/runs/38075902187/job/114282764024)
on October 10, 2026 took **3m35s**: default/integration plus Linux benchmark
smoke coverage took **37s**, and all-features tests (including doctests) took
**4s**. The approximately 3,147 MB workspace cache restored in **2m26s**.

For comparison, the same-day [all-Titan job for PR #101](https://github.com/titan-engine/titan/actions/runs/38064376148/job/114249100017)
changed the same leaf crate and took **15m28s**: default plus benchmark tests
**3m49s**, extra tests **8m42s**, cache restore **2m26s**. The selected job was
about 77% shorter overall in this sample. This is not a controlled benchmark:
source changes and cached feature footprints differ, and runner variability
still applies. The selective run rebuilt targets for **23.81s** initially,
**11.85s** for the separate benchmark invocation and **2.83s** for all features;
a cache hit did not eliminate feature-unification rebuilds. PR #105 is closed
without merging after the measurement; [PR #104](https://github.com/titan-engine/titan/pull/104)
retains the audit trail.

Selection reduces unrelated test targets, not necessarily all their dependency
compilation. Restored workspace artifacts may need rebuilding when package or
feature unification differs from the cached invocation, especially when moving
between default, all-features and headless recipes. Cache restore time and
runner variability remain fixed costs; timings should report the restored
cache and per-step durations, not imply a guaranteed speedup or timing target.
