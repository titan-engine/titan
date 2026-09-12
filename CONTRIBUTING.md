# Contributing

Titan is built by humans and AI agents. The same planning, implementation, and review expectations apply to both.

## Plan work in an issue

Agree on an issue's scope before implementation starts. A self-contained issue explains the problem or goal and its background, defines what is included and useful non-goals, states observable completion criteria and how to verify them, and links relevant documentation. Use clear, ordinary English. Do not rely on chat history, private conversations, local branch state, machine-specific paths, or unpublished notes.

Use a parent issue for a larger goal and sub-issues for reviewable pieces of work. Create sub-issue and blocking relationships with GitHub's native relationship controls. Mark an issue as blocking another only when the blocked work genuinely requires the other issue to be completed first. Do not repeat sub-issue or blocking relationships as lists or links in an issue body.

If new evidence changes the required scope, update the issue and agree on the revised scope instead of silently expanding a pull request.

## Use a feature branch

Do not work directly on `main`. Start each change on a feature branch based on the current `main` branch. Keep the branch focused on the agreed issue scope.

## Submit from a fork

For the usual fork-to-upstream workflow:

1. Fork `titan-engine/titan` on GitHub and clone your fork.
2. Add `https://github.com/titan-engine/titan.git` as the `upstream` remote.
3. Create a feature branch from the latest `upstream/main`.
4. Push the feature branch to your fork (`origin`).
5. Open a pull request from your fork's branch to `titan-engine/titan`'s `main` branch.

## Keep pull requests reviewable

Normally, a pull request addresses one primary issue and one coherent change. Keep intertwined changes together when splitting them would make the result harder to understand or leave it incomplete. There is no arbitrary pull-request line-count limit; clarity, scope, and evidence matter more than a number.

Explain the primary issue, what changed and why, how the change was verified, and any relevant limitations. When code changes behavior, commands, APIs, or requirements, update the relevant documentation in the same pull request. See the [development constraints](docs/development.md) for the project's technical and writing policies.
