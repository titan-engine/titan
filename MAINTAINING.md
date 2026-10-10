# Maintaining Titan

## Remotes

Maintainers' local checkouts use three remotes:

| Remote          | URL                                      | Purpose                              |
| --------------- | ---------------------------------------- | ------------------------------------ |
| `upstream-bevy` | `https://github.com/bevyengine/bevy`     | Fetch only. Bevy releases and fixes. |
| `upstream`      | `git@github.com:titan-engine/titan.git`  | The canonical Titan repository.      |
| `origin`        | `git@github.com:<you>/titan.git`         | Your fork, for PR branches.          |

To set up `upstream-bevy` so it only fetches `main` and release branches and
refuses pushes:

```sh
git remote add upstream-bevy https://github.com/bevyengine/bevy
git config remote.upstream-bevy.fetch '+refs/heads/main:refs/remotes/upstream-bevy/main'
git config --add remote.upstream-bevy.fetch '+refs/heads/release-*:refs/remotes/upstream-bevy/release-*'
git remote set-url --push upstream-bevy DISABLED
git config remote.pushDefault origin
```

## Tags

Bevy's tags (`v0.20.0`, ...) mark the upstream baselines we merged. Titan's own
releases use a separate namespace, `titan-v<major>.<minor>.<patch>`, so they
can never collide with Bevy tags. Don't push all of Bevy's tags to `upstream`.
Push only the release tag you're merging.

## Syncing a Bevy release

Titan's `main` is based on a stable Bevy release, never on Bevy `main`. To move
to a new release, e.g. 0.21.0:

```sh
git fetch upstream-bevy --tags
git fetch upstream
git switch -c sync/bevy-0.21.0 upstream/main
git merge --no-ff v0.21.0 -m "Merge Bevy v0.21.0"
# resolve conflicts, then:
cargo run -p ci
git push origin sync/bevy-0.21.0
git push upstream v0.21.0
```

Open a PR and land it with a **merge commit** (not a squash) so the Bevy
history stays connected. Update the version in `README.md` and `AGENTS.md`.

Bevy cuts releases from `release-*` branches that cherry-pick fixes from
Bevy `main`. Because of that, a release merge sometimes sees the same change
on both sides. Git usually resolves these cleanly, but take extra care with
conflicts in files Bevy patched late in a release cycle.

### Expected conflicts

These files differ from Bevy on purpose. On conflict, keep Titan's version
(and pick up any upstream changes that matter, such as new license notes):

- `README.md`, `CONTRIBUTING.md`, `SECURITY.md`
- `.github/ISSUE_TEMPLATE/config.yml`, `.github/pull_request_template.md`
- `.github/workflows/update-caches.yml` (checks out Titan instead of Bevy)

These Bevy files were deleted in Titan. If one shows a modify/delete conflict,
resolve it with `git rm <file>`:

- `.github/FUNDING.yml`, `.github/dependabot.yml`
- `.github/workflows/`: `action-on-PR-labeled.yml`, `ci-comment-failures.yml`,
  `docs.yml`, `example-run-report.yml`, `post-release.yml`, `release.yml`,
  `welcome.yml`

### Between releases

To catch divergence early, try merging Bevy `main` into a throwaway branch from
time to time and run CI. Don't merge that branch. It's only a warning signal.

Important fixes can be cherry-picked from Bevy (`git cherry-pick -x <sha>`)
before the next release.

## CI change scoping

`ci.yml`, `titan.yml`, `validation-jobs.yml` and `example-run.yml` classify PRs
and merge groups with `.github/scripts/ci-paths-changed.sh`, through the shared
`ci-changes` action. Each workflow needs its own cheap classifier job (outputs
cannot be shared across independent workflow runs). CI reuses the existing
`miri-changes` check name for this job.

Classification uses `git diff --name-only --no-renames -z BASE...HEAD`. The PR
base/head SHAs exclude unrelated changes on the base branch; merge groups use
GitHub's group base/head SHAs and include **every queued entry**. Deletions and
moves out of upstream paths still count. Job-level `if:` conditions report
skipped checks instead of leaving required checks pending. No required check
names or repository ruleset settings change.

| Changed paths | Coverage |
| --- | --- |
| Only `crates/titan_*/*` and/or `demos/*` | Titan default/all-feature tests, scoped `ci` and `check-doc`, text lints; scoped Windows/macOS tests in merge groups |
| Only standalone Markdown and/or `docs/*` | `markdownlint`, `toml`, `typos` |
| Titan paths plus standalone docs | Same coverage as Titan-only |
| Anything else, including Bevy code, `.github/*`, `.cargo/*`, root `Cargo.toml`/`Cargo.lock`, build configuration or unknown paths | Full coverage, including Titan consumers |

Upstream directories are checked **before** the Markdown rule: Markdown under
`crates/bevy_*`, `src`, `examples`, `benches`, `tools`, `errors`, `tests`,
`tests-integration`, `_release-content` or `release-content` triggers full
coverage because it may be generated or included by code. `docs/cargo_features.md` also
requires full coverage: it is generated and included by `src/lib.rs`.
Documentation inside Titan crates/demos gets Titan coverage too. These are allowlists: new top-level
areas default to full coverage until reviewed explicitly.

Missing SHAs, unavailable history, script errors and malformed outputs default
to full coverage. Consumer conditions also run on classifier-job failure or
missing output; cancellation still cancels the run. The action publishes its
selection in the job summary. A genuinely empty diff only needs the text lints.

Titan's fast path selects **all** `titan_*` workspace packages using Cargo
metadata, including demos, rather than only the changed package. This preserves
cross-crate consumer coverage and automatically includes new Titan packages.
`ci` keeps workspace rustfmt (cheap), but scopes all-target/all-feature Clippy.
`check-doc` scopes default-feature doctests and all-feature rustdoc, with warnings
denied. `titan-tests` adds scoped default-feature tests when the Bevy workspace
build is skipped, retaining its existing per-crate extra-feature/headless tests.
The Linux scoped `test` path also runs `cargo test <Titan packages> --benches`
with `--features bevy_remote/bevy_render`. The full CI driver's benchmark smoke
run has no explicit feature flags, but its workspace scope implicitly unifies
render; this separate scoped invocation must preserve that explicitly to avoid
HTTP-only dependency warnings. Benchmarks do not add `bevy_ecs/track_location`
or `--all-features`.
Windows/macOS still skip benchmark smoke tests, matching `--skip-benches` in the
full build recipe. `RUNNER_OS` selects this policy in Actions; local invocations
fall back to `uname -s`.
The `bevy_remote/bevy_render` feature matches workspace feature unification and
avoids upstream HTTP-only dead-code warnings. Dependencies still compile as
needed; their own tests and Bevy-wide platform builds are not run on this fast
path. Titan-only merge groups retain scoped Windows/macOS default-feature tests
under the existing `build (windows-latest)` and `build (macos-latest)` names.
Those two checks stay skipped for Titan-only PRs, as before; upstream-affecting
PRs run them in full. Normalize carriage returns at line-oriented tool-output
boundaries (native Windows `jq -r` emits CRLF under Git Bash) before assembling
package arguments or comparing classifier outputs. Do not normalize Git's
NUL-delimited paths: carriage returns can be part of a real filename.

Pushes to `main` and `release-*`, the daily scheduled runs, and manual dispatches
of these four workflows always run full coverage. Miri's daily coverage comes
from CI's schedule rather than a duplicate independent scheduled run. To force a full run before
merging, dispatch **each** workflow from Actions using the PR's branch (or
`gh workflow run <workflow-file> --ref <branch>` in the repository hosting that
branch). A re-run of a PR run keeps its original classification; it is not a
force-full override. Upstream-affecting PRs now run the same full job selection
as merge groups, including platforms previously deferred until the queue.
Screenshot comparisons still only run outside PRs, after their producer succeeds.
The independent security/dependency workflows keep their existing policy.
The existing cache-maintenance test matrix also checks scoped Cargo argument
construction with native jq and forced LF/CRLF output on Linux, macOS and
Windows, without adding jobs or compiling Rust. Its path list includes the
scoped script and regression test so parser-only changes exercise Windows in
PR CI even when the production scoped Windows job is skipped. Consolidating
these jobs is a separate follow-up, not part of this fast path.

Validate changes without compiling Rust:

```sh
bash .github/scripts/test-ci-paths-changed.sh
bash .github/scripts/test-miri-paths-changed.sh
bash .github/scripts/test-titan-scoped-ci.sh
actionlint -shellcheck= .github/workflows/{ci,titan,validation-jobs,example-run}.yml
shellcheck .github/scripts/{ci-paths-changed,test-ci-paths-changed,titan-scoped-ci,test-titan-scoped-ci}.sh
```

The tests cover Titan, upstream, mixed, docs, shared CI/build configuration,
unknown paths, deletions, renames, unusual filenames, diverged PR bases,
multiple merge-group entries and fail-safe behavior. They also execute the
composite action's actual selection shell block for diff/non-diff events,
script failures, malformed outputs and CRLF results, and verify scoped Cargo
command flags with both LF and CRLF metadata/jq output, including Linux-only
benchmark smoke selection and its absence on Windows/macOS. The three-OS
helper checks are permanent cheap steps in existing jobs, not temporary jobs.
Run real full CI when changing this shared infrastructure. To demonstrate the
fast path before it lands, open a temporary PR against a branch containing the
implementation in an Actions-enabled fork, with only a Titan-path change in
its diff. Do not weaken the `.github/*` full-coverage rule for the implementation
PR itself. Report runner-consuming jobs separately from skipped check records.

## Proposing changes upstream

Contributors who want to upstream a Titan change should cherry-pick it onto a
branch of their own Bevy fork and open the PR against `bevyengine/bevy`. Follow
Bevy's [contribution guide](https://bevy.org/learn/contribute/introduction) and
[AI policy](https://bevy.org/learn/contribute/policies/ai/).
