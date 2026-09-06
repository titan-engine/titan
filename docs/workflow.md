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
criteria and scope are clear, and its prerequisites are satisfied. Fill missing
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
implementation complete, using the accepted evidence described below. Linking a PR does not change status or ownership. Moving a card to Done does
not itself close its issue. Reopened work must be triaged explicitly. PRs appear
through the Linked pull requests field rather than duplicate execution cards.
Set In review explicitly when the linked implementation is ready.

## Maintainer-run agent implementation loop

1. Read the issue, dependencies, relevant design docs and applicable skills.
   Select a concrete, unblocked Ready issue with clear scope and checks.
   Claim Owner and set In progress before editing;
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
6. Enqueue work within the issue scope autonomously only after independent review,
   resolved review discussions, and all required PR checks pass for the current change.
   Do not update branches merely because main advanced: the queue tests the latest
   main plus preceding queued changes before merging. Address real conflicts and
   failures, with renewed review where needed. Never bypass the queue/protections
   or force-push main. Return scope changes and releases to the maintainer.
   An explicit request for maintainer review before merge takes precedence over
   autonomous enqueueing; leave that PR open until the review is complete.
7. Verify CI for the exact resulting main SHA, using full CI or the accepted
   exact-SHA queue evidence below. Link the SHA and accepted run in the PR/issue,
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

```sh
git worktree add ../titan-example -b codex/example main
# In the chosen worktree, for genuinely dependent layers:
gh stack init codex/foundation
gh stack add codex/integration
gh stack submit --auto --remote origin
gh stack view --json
# Enqueue only the reviewed, green range within issue scope:
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

## Exact-main verification and cache warming

A successful **Exact main revision verified** job records the full main SHA and
its accepted CI run in the job summary. Completion reports must link that SHA
and run. Successful PR-head CI alone is insufficient; a matching tree, ancestor,
or a queue run for another SHA is also insufficient. When the summary accepts
queue evidence, do not wait for a second full main execution. Browser demos
publication remains a separate main-only deployment with its existing build gates.

The main-push selector requires one unambiguous completed successful
`merge_group` run from this repository's `.github/workflows/ci.yml` workflow ID,
for the exact full main SHA. It checks the workflow contents at that SHA, the
latest run attempt, the **CI revision** job's successful step naming the executed
workflow SHA, and every expected required job and named verification step. The
revision job checks both `github.workflow_sha` and the checkout SHA. Missing,
pending, failed, cancelled, incomplete, ambiguous or inaccessible evidence means
full CI. A failed selector job also starts full CI; no empty output grants reuse.
The final result fails unless accepted queue evidence or the full fallback passes.

PR and queue required jobs are unconditional. The separate main workflow's
selection, summary and reusable full-suite job names do not replace their required
check names. No branch protection or queue rules are changed. The verification
commands are shared by calling the same `ci.yml` at the same commit; they do not
branch on event or ref. Cache scope, cancellation and diagnostic artifact names
can differ without changing verification. The selector deliberately rejects an
unrecognized gate layout or event-dependent verification. When changing the gate
contract (including parallel CI), update its extraction/tests before expecting
reuse; a contract it cannot prove continues to run full CI. Mutable runner images
and existing action aliases remain environmental variation, just as between two
full runs; this policy proves revision and command equivalence, not hermeticity.

Default-branch caches cannot be assumed to come from queue runs. Full verification
on cache-input changes (Cargo manifests/locks, pinned tool versions, workflows,
composite actions and CI cache tooling) warms main immediately. A weekly Monday
04:23 UTC full run refreshes default-branch caches; ordinary fallback runs warm
them too. `workflow_dispatch` on **Main verification** or **CI** at `main` forces
full verification and permits manual warming after eviction. Existing immutable
cache keys still control writes: a full run populates a missing key, not an
existing cache entry. Restore prefixes remain available between warmings. Browser
demos keeps its independent main build/deploy and cache policy.

Evaluate savings over a complete warming interval, including full warming/fallback
jobs and the selector/summary jobs, rather than counting only skipped suites.
For the historical same-SHA pair in issue #119, the
[queue run](https://github.com/titan-engine/titan/actions/runs/34026940070)
used 21.90 runner-minutes and the
[main run](https://github.com/titan-engine/titan/actions/runs/34027341007)
used 24.85 runner-minutes (sums of required job start/end durations); main required
wall time was 11m23s. These are baseline measurements, not measured savings of
this implementation. At those costs, `n` otherwise redundant main suites and `w`
additional warmings would save `24.85 × (n − w)` runner-minutes minus decision and
summary overhead; low merge frequency may yield no net saving. Record actual
run timings and cache effects before making a maintained performance claim.

Live rollout verification requires a merged revision containing this workflow:
link its actual queue run and main summary demonstrating reuse, then a forced
full main run demonstrating fallback/full execution. Reuse needs a push without
cache-input changes after the workflow has landed, because workflow changes
intentionally choose warming. Do not claim these live paths were demonstrated
by unit tests, a branch manual run, or historical CI runs. Keep issue #119 open
until this public evidence and interval measurements are recorded. A maintainer
review-before-merge request still takes precedence over collecting rollout evidence.

## CI and routine CLI use

CI runs in full on PRs, `merge_group: checks_requested`, and manual dispatch.
Main pushes use **Main verification** to select exact-SHA queue evidence or call
the same CI workflow in full. Scheduled and manual Main verification runs always
run the full suite. Feature-branch pushes do not
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

For the maintainer administration commands above, the authenticated CLI needs
`repo` and `project` scopes; organization visibility uses `read:org`.
Ordinary fork contributors do not need these project-administration permissions.
Built-in project workflows handle intake without copying a
personal token into Actions secrets. Never publish auth tokens or discovery
registrations in issues, reviews or evidence. No release follows automatically
from merging an issue or completing a project column.
