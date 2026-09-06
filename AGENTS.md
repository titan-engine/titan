# Titan agent workflow

Human contributors start with `CONTRIBUTING.md`. These instructions apply to
agents operating on the repository; they do not require outside contributors to
use agent tooling or administer the project board.

Read `docs/workflow.md` and the linked GitHub issue before implementation.
Pending work lives in https://github.com/orgs/titan-engine/projects/1.
Claim concrete Ready work with clear scope, acceptance criteria and satisfied
prerequisites; no separate issue approval is required. Record Owner and use an
isolated worktree plus a `codex/` branch.

Use subagents for substantial independent implementation and review. Keep
coherent local commits, submit reviewable batches through PRs, and continue
independent Ready work and eligible dependent stack layers while review, CI or
the queue runs. Follow docs/workflow.md for starting from available unmerged code,
true blocking prerequisites, coordinated ownership and necessary upstack rebases;
keep dependency links truthful. Never push directly to main or bypass
its required checks. Enqueue work within the issue scope autonomously after independent review
and green PR CI; the required merge queue validates integration before merging.
Do not refresh branches solely because main advanced. Review carry-forward and
conflict handling follow docs/workflow.md; scope changes and releases require maintainer input.
Honor explicit review-before-merge requests instead of enqueueing autonomously. Verify the exact
merged main revision's CI before reporting completion.

Agent review comments are authorized and must clearly say they are agent reviews,
including a public reviewer/task label, actual model name (or unavailable), UTC date/time,
full reviewed SHA, findings, verification and limitations. Do not publish private
session links or local worktree paths as review evidence. Maintainer-run agents currently
share one GitHub account; PR comments provide review evidence, not human approvals.

Use the gh-stack skill for dependent PR stacks; ordinary independent work uses
ordinary PRs. Keep brainstorming and planning in local conversations or GitHub
Discussions, never in tracked Markdown or another tracked format. Follow the
[planning and issue intake policy](docs/workflow.md#planning-and-issue-intake)
for concrete issues and accepted durable documentation. Read `docs/vision.md`, `docs/design-requirements.md` and relevant runtime
skills before engine changes. Quality gates are in `docs/verification.md`.
Preserve reference checksums and the committed crisp README preview unless an
intentional visual change is approved. Do not publish crates or create release
tags without authorization.
