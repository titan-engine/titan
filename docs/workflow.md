# GitHub-native development workflow

[Titan Development](https://github.com/orgs/titan-engine/projects/1) is the shared
backlog and execution board. [Issues](https://github.com/titan-engine/titan/issues)
own pending work; repository docs own durable requirements, architecture, usage
and verification. Do not maintain a second TODO backlog in Markdown. Capture new
ideas and discovered follow-up work as issues, linking source requirements.
Small implementation steps may remain checklists within their owning issue.

## Approval and ownership

| Status | Meaning |
| --- | --- |
| Proposed | Captured idea or bug; implementation is not approved. |
| Ready | User-approved scope with concrete acceptance criteria. Check dependencies before claiming. |
| In progress | One owner has claimed the approved issue and started work. |
| In review | Implementation is ready for independent review and required CI. |
| Done | Work is completed and its implementation merged. Verify the resulting main revision before reporting completion. |

Ready is the approved queue, not a promise that prerequisites are complete. The
Ready view excludes issues GitHub marks blocked. Priority does not grant approval.
Record the user-approved scope and source of approval in the issue before moving
it to Ready. A broad requirement or old planning answer does not authorize its
implementation. Split broad proposals into bounded sub-issues when selected.
Blank issues and CLI-created issues are allowed for quick idea capture. Before
moving either to Ready, add the outcome, acceptance/verification criteria, scope
boundaries, dependencies and recorded user approval required by the work template.

Use Priority (P0 urgent, P1 high, P2 normal, P3 later), Area and Owner fields.
Owner identifies the responsible agent/task, since agents share a GitHub account;
Assignees identify GitHub users. Labels classify work: bug, enhancement,
investigation, maintenance, documentation, or tracking. Existing GitHub triage
labels remain available. Use sub-issues for decomposition and blocking
relationships only for actual prerequisites. Related work need not be blocked.

New/updated open Titan issues automatically enter the project and default to
Proposed. Sub-issues are automatically included. Closing an issue sets Done;
linking a PR does not change approval/ownership status. Moving a card to Done does
not itself close its issue. Reopened work must be triaged explicitly. PRs appear
through the Linked pull requests field rather than duplicate execution cards.
Set In review explicitly when the linked implementation is ready.

## Implementation loop

1. Read the issue, dependencies, relevant design docs and applicable skills.
   Select an unblocked Ready issue. Claim Owner and set In progress before editing;
   coordinate claims through one integration owner to avoid simultaneous claims.
2. Use an isolated worktree and a `codex/` branch for each independent change.
   Keep two or three independent implementations active initially. Agree shared
   interfaces before parallel work; avoid concurrent ownership of the same files.
3. Commit coherent increments locally. Open a linked draft PR to expose work and
   dependencies; push reviewable batches rather than every small local edit.
4. Run relevant local gates, update affected usage/design docs, and collect
   native/browser/headless evidence for runtime changes. Preserve reference images
   and checksums unless an intentional visual change was approved.
5. Obtain independent agent review, address findings, and move the issue to
   In review. Record the review on the PR using the attribution format below.
6. Enqueue approved scope autonomously only after independent review, resolved
   review discussions, and all required PR checks pass for the current change.
   Do not update branches merely because main advanced: the queue tests the latest
   main plus preceding queued changes before merging. Address real conflicts and
   failures, with renewed review where needed. Never bypass the queue/protections
   or force-push main. Return scope changes and releases to the user.
7. Verify CI for the exact resulting main SHA. Link the result in the PR/issue,
   ensure the issue is completed, and release its worktree when no longer needed.
   While checks run, continue another eligible issue if independent work exists.

GitHub protects main with PR requirements, a required merge queue, resolved
conversations and no force pushes/deletion, including for admins. Legacy branch
protection retains the required checks with strict branch freshness disabled; the
active `Main merge queue` ruleset requires queued integration with no bypass actors. Required jobs
are Native checks, WebAssembly core check, and macOS development app bundles.
GitHub approval count is zero because all agents currently share the author's
account and cannot cast independent approval votes. Independent review is a
mandatory workflow rule recorded as PR evidence; GitHub does not enforce reviewer
independence. Revisit approval rules when separate reviewer identities exist.

## Attributed agent reviews

The user authorizes agents to comment on PRs for review. Clearly identify each
review as agent-generated, never as a human approval. Include actual available
model identity; do not invent a model/version when unavailable. Example:

```text
Agent review
Reviewer: <agent/task identity>
Model: <actual model name, or unavailable>
Reviewed at: <YYYY-MM-DD HH:MM:SS UTC>
Reviewed commit: <full PR head SHA>
Scope: <files/behavior examined>
Findings: <actionable findings, or no material findings>
Verification: <checks personally performed and evidence inspected>
Limitations: <anything not verified>
Disposition: <changes requested / ready within approved scope>
```

The author cannot substitute self-review for independent review. An integration
owner may post another agent's review with explicit attribution. Review material
changes made after the reviewed SHA again; do not treat an earlier clean review
as covering unseen changes. Use `--body-file` for exact multiline comments.

Review covers authored changes; queue CI covers their integration with current
main. A queue-generated commit does not change the PR head or require another
full authored-code review. If a branch is rebased or merged with main without
changing its authored diff, an integration owner may carry forward the independent
review after comparing old/new diffs (for example with `git range-diff`) and
checking relevant main changes for semantic interactions. Post attributed evidence
with old/new full head SHAs, comparison performed, and why review still applies.
Do not assume a conflict-free merge proves semantic equivalence. Conflict
resolutions, substantive edits, or relevant changed assumptions need independent
review of the affected changes before re-enqueueing. When uncertain, obtain review.

## Branches and stacks

Use ordinary PRs for independent changes. Use short `gh stack` stacks when later
code depends on an unmerged foundation; separate concerns before writing them.
Read the installed gh-stack skill for operations and its stack-design reference
before creating a stack. Each layer must build and carry its relevant tests/docs.
Keep independent work on separate branches/stacks, not artificial dependency chains.

```sh
git worktree add ../titan-example -b codex/example main
# In the chosen worktree, for genuinely dependent layers:
gh stack init codex/foundation
gh stack add codex/integration
gh stack submit --auto --remote origin
gh stack view --json
# Enqueue only the reviewed, green, approved range:
gh stack merge <top-PR-number> --yes
```

Stack creation commands above illustrate alternatives to a normal independent
branch, not a requirement to stack every change. Use `gh stack` to rebase/sync
stack branches and merge a stack; do not use `gh pr merge` for stacks. Never
rewrite a branch another owner is editing without coordination. Ordinary PRs can
use `gh pr merge <PR-number> --match-head-commit <reviewed-SHA>` after PR
checks pass; this enqueues rather than bypasses integration. Do not use `--admin`.
For stacks, `gh stack merge` queues the selected range and the queue chooses the
merge method. Queued stack layers may land in separate groups; do not promise
atomic landing of an entire stack.

The CLI extension is installed and this checkout sets `rerere.enabled=true` and
`remote.pushDefault=origin`. New clones need equivalent local setup. Stacks remain
a GitHub preview; no stack is needed for the workflow setup's single PR.

## Queue configuration and operation

The main queue uses squash merging, at most three concurrent integration builds,
`ALLGREEN` (every queue entry must pass), groups of one to three PRs, and a
60-minute check timeout. The minimum group size is one, so there is no batching
wait (the configured one-minute minimum-group wait is inactive at that minimum).
Merge limits control landing groups, not how many CI builds are combined.

Keep all three required jobs on PRs and merge groups. Queue CI validates current
main plus earlier queued work; it does not eliminate integration testing or
rebuilds after failed entries. Monitor the PR/queue and inspect the failed run
and removal reason before retrying. Fix code or conflicts on the owning branch,
review affected changes and pass PR checks before re-enqueueing. Retry a transient
infrastructure failure only with evidence. Do not routinely jump the queue or
remove/re-add entries: reordering can invalidate other builds. CodeRabbit remains
optional and is not a queue requirement.

When administering the queue, ensure the CI trigger exists before enqueueing work.
Keep the queue requirement active before disabling legacy strict branch freshness;
never remove required checks to make an entry merge. Inspect both legacy branch
protection and active rulesets when diagnosing blocked integration.

## CI and routine CLI use

CI runs on PRs, `merge_group: checks_requested`, pushes to main, and manual dispatch. Feature-branch pushes do not
also start a duplicate push run. Superseded runs cancel only within the same PR;
main and merge-group runs remain independent. Build/download caches are separated by platform,
job and toolchain, with manifest/lockfile keys. Runtime diagnostic/discovery data
is excluded. Cache misses affect speed, not whether verification runs. All stack
layers retain full required gates; no stack-top-only exception is configured.

```sh
gh issue create --title 'Concrete outcome' --body-file /tmp/issue.md --label investigation
gh issue edit <issue> --add-blocked-by <prerequisite>
gh issue view <issue> --json number,title,body,blockedBy,blocking,projectItems
gh project item-list 1 --owner titan-engine --limit 100 --format json
gh project field-list 1 --owner titan-engine --format json
# Use returned item/field/option IDs for explicit status/owner updates:
gh project item-edit --id <item-id> --project-id <project-id> --field-id <status-field-id> --single-select-option-id <option-id>
gh pr create --draft --title 'Concrete outcome' --body-file /tmp/pr.md
gh pr comment <pr> --body-file /tmp/agent-review.md
gh pr checks <pr>
```

The authenticated CLI needs `repo` and `project` scopes; organization visibility
uses `read:org`. Built-in project workflows handle intake without copying a
personal token into Actions secrets. Never publish auth tokens or discovery
registrations in issues, reviews or evidence. No release follows automatically
from merging an issue or completing a project column.
