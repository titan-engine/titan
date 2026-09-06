# Maintainer and agent workflow

For your first contribution, start with [CONTRIBUTING.md](../CONTRIBUTING.md).
This document covers project administration and maintainer-run agents. Outside
contributors use ordinary forks and PRs; they do not need board write access,
agent tooling, multiple worktrees, or permission to operate the merge queue.
The maintainer coordinates those steps for them.

[Titan Development](https://github.com/orgs/titan-engine/projects/1) is the shared
backlog and execution board. [Issues](https://github.com/titan-engine/titan/issues)
own pending work; repository docs own durable requirements, architecture, usage
and verification. The planning and intake policy below governs the handoff
from discussion to issues; do not maintain a second TODO backlog in repository files.

## Planning and issue intake

Keep brainstorming and planning in local conversations with the maintainer and
agents, or in [GitHub Discussions](https://github.com/titan-engine/titan/discussions)
when wider collaborators are involved. Agents must not save plans to tracked
Markdown or any other tracked format. Repository docs may record accepted durable
requirements, architecture, usage, verification procedures and decision rationale.
They must not contain scratch plans, candidate issue specifications, speculative
roadmaps, task journals or alternate backlogs. Do not publish private conversations
as part of the handoff.

Once discussion yields concrete work, create a well-specified issue recording:

- The intended outcome and problem it addresses.
- Acceptance criteria and how to verify them.
- Scope boundaries, including what is excluded.

Triage actual prerequisites using native GitHub blocking relationships, and use
native parent/sub-issue relationships for decomposition. These relationships are
authoritative: do not duplicate their lists or titles in issue bodies. Add prerequisite
rationale in prose only when it explains something beyond the native relationships;
no body list or "none known" declaration is required. Maintainers handle relationship
updates for contributors without access.

Link public source discussions or accepted requirements when relevant. Concrete
work enters **Ready** without a separate proposal stage or maintainer approval
record. Blank issues and CLI-created issues follow the same intake policy; use
conversations or Discussions for ideas that are not yet concrete. Discovered
follow-up work follows this policy too. Small implementation steps may remain
checklists within their owning issue.

Ordinary bug reports do not require this completed specification: report observed
behavior, reproduction details and environment as available. Ask usage or contributor
questions in Discussions. Reporters need not design a fix or perform maintainer
triage; maintainers fill in missing criteria and boundaries, triage native dependency
relationships before implementation begins.

For example, explore possible rendering features in a local conversation; discuss
a wider API proposal in Discussions; document an accepted rendering architecture
and its rationale in repository docs. Submit a crash report as a bug even without
a proposed fix. Turn a selected implementation or bounded investigation into an
issue with the outcome, checks and boundaries above, and triage
its dependencies through native GitHub relationships.

## Status and ownership

| Status | Meaning |
| --- | --- |
| Ready | Queued work. Check scope, acceptance criteria and dependencies before claiming; fill gaps during bug triage. |
| In progress | One owner has claimed the issue and started work. |
| In review | Implementation is ready for independent review and required CI. |
| Done | Issue is closed: implemented, not planned or duplicate. The closure reason distinguishes retired work from completed implementation. |

Ready is the queue; it does not guarantee that prerequisites are complete or that
a newly filed bug has been fully triaged. The Ready view excludes issues GitHub
marks blocked. Before claiming, check that the work is concrete, its acceptance
criteria and scope are clear, and its prerequisites are satisfied for the work being started (see
[dependent implementation](#overlapping-dependent-implementation) for available
unmerged code). Fill missing
details through triage, without a separate approval step. Priority orders work.
Broad requirements and historical design discussions remain context; turn selected
work into bounded issues under the [planning and issue intake policy](#planning-and-issue-intake).

Use Priority (P0 urgent, P1 high, P2 normal, P3 later), Area and Owner fields.
Assignees identify contributors. Owner is optional coordination metadata for
maintainer-run agents sharing a GitHub account; keep private session identifiers
out of public fields. Outside contributors comment to request an issue and the
maintainer records the assignment. Labels classify work: bug, enhancement,
investigation, maintenance, documentation, or tracking. Use `good first issue`
for bounded beginner tasks with starting points and verification, and `help wanted`
for work seeking contributors. These labels do not override status or dependencies.
Use sub-issues for decomposition and blocking
relationships only for actual prerequisites. Related work need not be blocked.

New/updated open Titan issues automatically enter the project and default to
Ready. Sub-issues are automatically included. Closing an issue sets Done; use the
closure reason to distinguish implemented work from work retired as not planned
or duplicate. Verify the exact resulting main revision before reporting an
implementation complete. Linking a PR does not change status or ownership. Moving a card to Done does
not itself close its issue. Reopened work must be triaged explicitly. PRs appear
through the Linked pull requests field rather than duplicate execution cards.
Set In review explicitly when the linked implementation is ready.

## Maintainer-run agent implementation loop

1. Read the issue, dependencies, relevant design docs and applicable skills.
   Select concrete Ready work with clear scope and checks. Apply the
   [dependent implementation rules](#overlapping-dependent-implementation) when
   an implementation dependency is available in an unmerged layer.
   Claim Owner and set In progress before editing;
   coordinate claims through one integration owner to avoid simultaneous claims.
2. Use an isolated worktree and a `codex/` branch for each independent change.
   Keep two or three independent implementations active initially; also advance
   eligible dependent layers while lower layers are in review, CI or the queue. Agree shared
   interfaces before parallel work; avoid concurrent ownership of the same files.
3. Commit coherent increments locally. Open a linked draft PR to expose work and
   dependencies; push reviewable batches rather than every small local edit.
4. Run relevant local gates, update affected usage/design docs, and collect
   native/browser/headless evidence for runtime changes. Preserve reference images
   and checksums unless an intentional visual change was approved.
5. Obtain independent agent review, address findings, and move the issue to
   In review. Record the review on the PR using the attribution format below.
6. Enqueue work within the issue scope autonomously only after independent review,
   resolved review discussions, and all required PR checks pass for the current change.
   Do not update branches merely because main advanced: the queue tests the latest
   main plus preceding queued changes before merging. Address real conflicts and
   failures, with renewed review where needed. Never bypass the queue/protections
   or force-push main. Return scope changes and releases to the maintainer.
   An explicit request for maintainer review before merge takes precedence over
   autonomous enqueueing; leave that PR open until the review is complete.
7. Verify CI for the exact resulting main SHA. Link the result in the PR/issue,
   ensure the issue is completed, and release its worktree when no longer needed.
   While checks run, continue another eligible issue if independent work exists.

GitHub protects main with PR requirements, a required merge queue, resolved
conversations and no force pushes/deletion, including for admins. Legacy branch
protection retains the required checks with strict branch freshness disabled; the
active `Main merge queue` ruleset requires queued integration with no bypass actors. Required jobs
are Native checks, WebAssembly core check, and macOS development app bundles.
GitHub approval count is zero because maintainer-run agents currently share the author's
account and cannot cast independent approval votes. Independent review is a
mandatory workflow rule recorded as PR evidence; GitHub does not enforce reviewer
independence. Outside contributions receive maintainer-coordinated review, which
may be human or clearly attributed agent review. Maintainer-run agent changes
require independent agent review. Revisit approval rules when separate reviewer
identities exist; changing this documentation does not alter GitHub protections.

## Attributed agent reviews

Independent review includes repository maintenance and reviewability. For
substantial diffs, require a concise explanation of why the parts belong together
and what accounts for their size, distinguishing authored source/docs, lockfiles,
necessary replay/golden fixtures and recorded output. Identify the permanent
consumer/purpose of newly retained artifacts: a test, current guide or maintained
claim. Classify by use, not extension; JSON can be an authored input or fixture.

Assess unnecessary historical output, duplicate current guidance and whether
independent concerns should be separated. Before declaring a change ready,
require unexplained bulk run output to be removed or justified under the
[evidence lifecycle](acceptance-evidence.md), and record retention findings in the
attributed review. This is part of existing independent review, with no blanket
line cap, size-based maintainer permission requirement or additional CI gate.

Maintainer-run agents are authorized to comment on PRs for review. Clearly identify each
review as agent-generated, never as a human approval. Include actual available
model identity; do not invent a model/version when unavailable. Example:

```text
Agent review
Reviewer: <public agent/task label, without private session links>
Model: <actual model name, or unavailable>
Reviewed at: <YYYY-MM-DD HH:MM:SS UTC>
Reviewed commit: <full PR head SHA>
Scope: <files/behavior examined>
Findings: <actionable findings, or no material findings>
Verification: <checks personally performed and evidence inspected>
Limitations: <anything not verified>
Disposition: <changes requested / ready within issue scope>
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

### Overlapping dependent implementation

An implementation dependency can be available before its issue closes: a lower
layer provides the code and an agreed interface that the next layer can build
against. Start that dependent issue without waiting for the lower PR to merge
when its scope and checks are concrete, the interface has been inspected and is
settled enough to build on, and all other start prerequisites are satisfied.
Review and CI may still be running on the lower layer. This permits overlapping
implementation, not early integration or assuming that pending checks will pass.

A true start prerequisite still blocks work: an unsettled contract, required
runtime evidence or maintainer decision, or an issue-specific requirement that
the prerequisite merge or complete first. For example, a consumer can use agreed
types in an unmerged foundation; it cannot choose a persistence format while the
format decision is outstanding, or start a rollout explicitly gated on a merged
migration. Work on an independent Ready issue while those prerequisites remain.

Keep native GitHub dependency links truthful until the dependency is resolved;
never remove them to expose a card in the Ready view. A dependent issue may be
excluded by that view even though implementation can start under these rules.
Inspect its linked prerequisite directly, then record Owner and In progress for
the dependent issue, including a short explanation of the available lower PR and
agreed interface in its claim. Leave the lower issue's ownership and status to
its owner. An integration owner coordinates the stack, shared interfaces and
which owner may submit or rebase each branch. If another agent owns the lower
layer, agree the base commit and handoff before starting; use isolated worktrees
and never rebase or rewrite another owner's active branch without coordination.

For example, one owner can build a two-layer inspection feature: protocol types
in the foundation, then a CLI consumer. Each layer includes its own relevant
tests and documentation. Start from current main in an isolated worktree; these
commands illustrate a new stack, not operations on another owner's branches:

```sh
git worktree add ../titan-inspection -b codex/inspection/protocol main
cd ../titan-inspection
gh stack init codex/inspection/protocol
# Implement and validate protocol types; stage only this layer's files.
git add <protocol-files>
git commit -m "Add inspection protocol types"
gh stack submit --auto --remote origin
# Request independent review of the foundation; its CI can run concurrently.
gh stack add codex/inspection/cli
# Implement and validate the CLI against the agreed foundation, without waiting
# for its PR to merge. Stage this layer's source, tests and documentation.
git add <cli-files>
git commit -m "Consume inspection protocol in CLI"
gh stack submit --auto --remote origin
gh stack view --json
```

Replace angle-bracket placeholders with actual files or PR numbers. Submit
coherent reviewable batches, edit the generated PR descriptions to explain each
layer, and obtain an attributed independent review of each authored diff against
its parent. Mark reviewed PRs ready with `gh pr ready <pr-number>` and retain full
required checks on every layer and merge group. When only the foundation is
reviewed and green, it alone is eligible; when both layers are reviewed and green,
the range through the CLI is eligible. Confirm the selected range from
`gh stack view --json` before enqueueing:

```sh
# Only after review, current checks and resolved discussions for every included PR:
gh stack merge <highest-eligible-pr-number> --yes
```

The command includes every unmerged layer below the selected PR. The installed
CLI resolves a bare number as a stack number before a PR number; check for that
ambiguity and do not issue the command if it would select a wider range. Never
include a draft, failing or unreviewed layer. Honor explicit review-before-merge
requests by leaving the affected range open, without enqueueing. The queue
chooses the merge method and may land layers in separate groups; stack landing
is not promised to be atomic. Ordinary independent PRs use
`gh pr merge <PR-number> --match-head-commit <reviewed-SHA>` after their checks
pass. Never use `--admin` or bypass protections; use `gh stack merge` for stacks.

### Lower-layer fixes and review carry-forward

If foundation CI or review finds a defect, fix and commit it on the foundation
branch. Continue upper-layer work only where the interface remains usable; an
invalidated contract blocks dependent work until it is settled again. Coordinate
a pause/handoff with affected owners before a necessary upstack rebase, with
clean worktrees and saved old parent/head SHAs for each layer. From the corrected
lower branch, the integration owner can run:

```sh
gh stack rebase --upstack --no-trunk --remote origin
```

This propagates the foundation change without refreshing from main. Compare each
old and new authored range, for example with
`git range-diff <old-parent>..<old-head> <new-parent>..<new-head>`, and inspect the
lower-layer change for semantic interactions. Apply the attributed review
carry-forward rules above: equivalent authored diffs alone do not prove an
unchanged dependency contract. Independently review the foundation fix and any
substantive upper changes or conflict resolutions; if equivalence or interactions
are uncertain, obtain renewed review. Publish the coordinated result as a
reviewable batch with `gh stack submit --auto --remote origin`, and require current
checks for every affected PR before enqueueing. A failed lower layer prevents
its entire dependent range from enqueueing even if upper checks passed earlier.

Use `gh stack` for necessary stack rebase/sync operations. Do not rebase merely
because main advanced, and avoid tiny repeated pushes that cancel useful CI.
Queue CI tests integration with current main; completion still follows the exact
merged-main verification policy above.

The stack extension is needed only for dependent stacks. Install it explicitly
when using that workflow; it is not a prerequisite for ordinary contributions.
For maintainer checkouts, `git config rerere.enabled true` can reuse recorded
conflict resolutions, and `git config remote.pushDefault origin` selects the
intended push remote. These settings are local to each checkout; choose the push
remote deliberately when working from a fork. Stacks remain a GitHub preview.

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
remove/re-add entries: reordering can invalidate other builds.

When administering the queue, ensure the CI trigger exists before enqueueing work.
Keep the queue requirement active before disabling legacy strict branch freshness;
never remove required checks to make an entry merge. Inspect both legacy branch
protection and active rulesets when diagnosing blocked integration.

## CI and routine CLI use

CI runs on PRs, `merge_group: checks_requested`, pushes to main, and manual dispatch. Feature-branch pushes do not
also start a duplicate push run. Superseded runs cancel only within the same PR;
main and merge-group runs remain independent. Independent workspace, game and copied-project workloads run on separate runners.
The three existing required check names are aggregate gates: every necessary
shard must succeed, including after upstream failures or cancellation. All stack
layers retain full required gates; no stack-top-only exception is configured.
See [CI workload and cache verification](verification.md#ci-workloads-and-cache-measurement)
for the command map, cache boundaries, refresh policy and timing procedure.

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

For the maintainer administration commands above, the authenticated CLI needs
`repo` and `project` scopes; organization visibility uses `read:org`.
Ordinary fork contributors do not need these project-administration permissions.
Built-in project workflows handle intake without copying a
personal token into Actions secrets. Never publish auth tokens or discovery
registrations in issues, reviews or evidence. No release follows automatically
from merging an issue or completing a project column.
