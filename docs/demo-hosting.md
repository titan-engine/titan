# Public browser demos

The public site is designed for <https://titan-engine.github.io/titan/>. It contains
the RPG and arena players and their paused inspectors, plus the 3D collection-room
player at `collection-room/play/`, with a short landing page linking to the source,
contribution guide and questions. Publication requires the Pages workflow to merge; a PR build alone does not publish a site.
The demo tracks `main`, so it is an experimental preview rather than a release.

## Build and preview

Use the same Rust/Cargo, Python 3 and Node.js prerequisites as the
[browser adapters](browser.md). No frontend package manager is needed.

```sh
python3 scripts/test-pages.py
python3 scripts/build-pages.py
node scripts/test-browser.mjs
node games/arena/scripts/test-browser.mjs
node games/collection-room/scripts/test-browser.mjs
python3 -m http.server 8000 --bind 127.0.0.1 --directory target
```

Open <http://127.0.0.1:8000/pages/>. Previewing under `/pages/` exercises the relative
links and assets needed by the deployed `/titan/` project URL. Open and reload all
three player routes directly, and try the RPG and arena paused inspectors.
The full build invokes each game's existing browser builder; the Pages workflow
runs all three actual-WASM acceptance scripts before uploading the package.

For the [collection room](../games/collection-room/README.md#play-in-a-browser),
open `/pages/collection-room/play/`, click Play and focus the canvas. Use WASD or
arrows to move, Space to pause/resume, N to step and R to restart. Click Replay
route and verify completion with three collected objects at game tick 44; test
Restart and Capture, including the displayed image and tick/revision identity.
Repeat with `?backend=webgpu` and `?backend=webgl2` on available supported adapters.
WebGL2 needs floating-point color attachments. Record the browser, adapter,
backend and any unsupported-capability error; a Node/WASM pass does not verify GPU
rendering. Reload should start a fresh session with inspector control disabled.

GPU play requires a compatible browser/device. Only RPG and arena offer on-screen
movement and paused inspectors with software captures. Collection room requires
a keyboard and GPU, has no standalone inspector page and no software 3D fallback.
Unsupported GPU capability must show the player's existing error UI.

`--no-build` restages existing compiled browser packages for layout iteration.
Do a full build before publishing. The script replaces only `target/pages`,
using an explicit list of HTML, JavaScript, WASM and game PNG files plus the two
licenses. It does not copy the checkout, arbitrary files from `web`, native
binaries, runtime discovery registrations, bearer tokens or diagnostic bundles.
Generated packages and the staged site remain ignored build outputs.
Packaging checks reject symlinked source files or parent directories and verify
that failed packaging preserves the previous output. The build job is bounded
to 45 minutes and the deployment job to 10 minutes.

## Build caches

The Browser demos workflow caches Cargo downloads and the host `target/release`
and `target/wasm32-unknown-unknown/release` directories for the root RPG, arena
and collection room. These are the directories used by the shared browser build
helper. Cache identity includes the runner OS/architecture, Rust compiler and
runner image identity, release/WASM profile, and Cargo manifests, lockfiles,
configuration and toolchain file. A UTC date suffix allows at most one new immutable snapshot per day and
branch/dependency set, limiting storage churn while periodically refreshing build
outputs. Later revisions restore only snapshots with matching dependency inputs.
Same-day source changes rebuild against the saved snapshot without replacing it. A change
to those inputs starts a cold compilation cache. This cache is separate from
engine CI, and GitHub's branch visibility and eviction rules can also cause misses.

Matching `wasm-bindgen` installations under each project's `target/titan/tools`
have a separate platform/toolchain/lockfile cache. Before reuse, the helper checks
the executable version against resolved Cargo metadata, installing the exact
version when needed. Standalone games can reuse the root engine's matching CLI.

Every run still invokes Cargo with `--locked`, regenerates browser and Node
bindings, stages the allowlisted site from scratch and executes packaging and all
three compiled-game checks. Cargo checks source changes after restoration; a
cache hit never skips building or testing. Generated web packages, `target/pages`,
Node bindings, captures and runtime/private data are outside the cached paths.
Only the freshly verified site is uploaded, so restoring a cache does not restore
a previously published package. Cache upload/download costs and eviction mean
the speedup depends on cache availability and dependency stability; compare the
build step together with cache restore/save time when measuring it.

## GitHub Pages administration

A maintainer selects **Settings → Pages → Build and deployment → Source → GitHub
Actions** once. Keep the `github-pages` environment restricted to `main`.
[GitHub's custom workflow guide](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
describes the hosting setup and required permissions.

[The workflow](../.github/workflows/pages.yml) builds and tests the public package
on PRs and merge groups with read-only repository permissions. Only pushes to
`main`, or manual dispatches selecting `main`, can deploy. The separate deployment
job has `pages: write` and `id-token: write`; it does not check out or execute
repository code. No personal token, custom domain, analytics or external backend
is required. Existing main protection and engine CI gates remain in place.

After a reviewed merge, check the **Browser demos** workflow's deployment URL and
record the exact merged-main SHA, its engine CI result and successful deployment
run. Open the public HTTPS landing page and all three players, repeat the smoke
checks above under `/titan/`, and record results against that deployed revision.
These post-merge checks remain pending while a PR awaits maintainer review.
If hosting fails, inspect that run; a green PR package build is not evidence of a successful deployment. To republish
the current main revision, use **Actions → Browser demos → Run workflow → main**.
Keep fixes in reviewed PRs rather than editing the generated site directly.
