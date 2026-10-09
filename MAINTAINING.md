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

## Proposing changes upstream

Contributors who want to upstream a Titan change should cherry-pick it onto a
branch of their own Bevy fork and open the PR against `bevyengine/bevy`. Follow
Bevy's [contribution guide](https://bevy.org/learn/contribute/introduction) and
[AI policy](https://bevy.org/learn/contribute/policies/ai/).
