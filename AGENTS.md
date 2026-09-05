# Titan contributor workflow

Read `docs/workflow.md` and the linked GitHub issue before implementation.
Pending work lives in https://github.com/orgs/titan-engine/projects/1.
Proposed issues are unapproved. Only claim approved Ready work with satisfied
prerequisites; record Owner and use an isolated worktree plus a `codex/` branch.

Use subagents for substantial independent implementation and review. Keep
coherent local commits, submit reviewable batches through PRs, and continue
independent approved work while CI runs. Never push directly to main or bypass
its required checks. Merge approved scope autonomously after independent review
and green CI; scope changes and releases require user input. Verify the exact
merged main revision's CI before reporting completion.

Agent review comments are authorized and must clearly say they are agent reviews,
including reviewer/task identity, actual model name (or unavailable), UTC date/time,
full reviewed SHA, findings, verification and limitations. All agents currently
share one GitHub account; PR comments provide review evidence, not human approvals.

Use the gh-stack skill for dependent PR stacks; ordinary independent work uses
ordinary PRs. Keep requirements/architecture/usage in repository docs and TODOs
in issues. Read `docs/vision.md`, `docs/design-requirements.md` and relevant runtime
skills before engine changes. Quality gates are in `docs/implementation-plan.md`.
Preserve reference checksums and the committed crisp README preview unless an
intentional visual change is approved. Do not publish crates or create release
tags without authorization.
