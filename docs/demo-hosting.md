# Public browser demos

The public site is designed for <https://titan-engine.github.io/titan/>. It contains
both the RPG and arena players and their paused inspectors, with a short landing
page linking to the source, contribution guide and questions. Initial publication
requires the Pages workflow to merge; a PR build alone does not publish a site.
The demo tracks `main`, so it is an experimental preview rather than a release.

## Build and preview

Use the same Rust/Cargo, Python 3 and Node.js prerequisites as the
[browser adapters](browser.md). No frontend package manager is needed.

```sh
python3 scripts/test-pages.py
python3 scripts/build-pages.py
node scripts/test-browser.mjs
node games/arena/scripts/test-browser.mjs
python3 -m http.server 8000 --bind 127.0.0.1 --directory target
```

Open <http://127.0.0.1:8000/pages/>. Previewing under `/pages/` exercises the relative
links and assets needed by the deployed `/titan/` project URL. Try each player,
its inspector and the RPG reference replay. GPU play requires a compatible
browser/device; paused inspectors render software captures. A browser failure
should show its existing retry/error UI rather than a blank playable world.

`--no-build` restages existing compiled browser packages for layout iteration.
Do a full build before publishing. The script replaces only `target/pages`,
using an explicit list of HTML, JavaScript, WASM and game PNG files plus the two
licenses. It does not copy the checkout, arbitrary files from `web`, native
binaries, runtime discovery registrations, bearer tokens or diagnostic bundles.
Generated packages and the staged site remain ignored build outputs.
Packaging checks reject symlinked source files or parent directories and verify
that failed packaging preserves the previous output. The build job is bounded
to 45 minutes and the deployment job to 10 minutes.

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
open both games on the public HTTPS site. If hosting fails, inspect that run;
a green PR package build is not evidence of a successful deployment. To republish
the current main revision, use **Actions → Browser demos → Run workflow → main**.
Keep fixes in reviewed PRs rather than editing the generated site directly.
