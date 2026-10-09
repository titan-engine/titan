# Contributing to Titan

Titan welcomes contributions, including ones written mostly or entirely by AI.
The bar isn't who or what wrote the code. It's whether the change is
**accountable, explained, and verified**.

## AI policy

AI-assisted and agent-authored work is welcome. Every merged change needs:

1. **An accountable human.** Whoever opens the PR, or sponsors an agent that
   opened it, answers review questions and fixes follow-up problems. You don't
   have to have typed every line, but you do have to stand behind it.
2. **A clear rationale.** The PR description says what problem this solves and
   why this approach was taken. AI-written descriptions are fine; vague or
   padded ones aren't.
3. **Verification proportional to risk.** Tests, reproduction cases, benchmarks,
   or screenshots as appropriate. "It compiles" is not verification.

### Disclosure

Say how AI was used in the PR template's **AI usage** section, for example
"Claude Code wrote the implementation; I reviewed it and wrote the tests." This
is not a stigma. It helps reviewers calibrate, and it's required if the change
is ever proposed to Bevy.

### Extra care required

These areas get stricter review and usually need a maintainer's sign-off before
large changes:

- `unsafe` code, and ECS internals (`bevy_ecs`, `bevy_ptr`, `bevy_tasks`)
- rendering (`bevy_render`, `bevy_pbr`, `bevy_core_pipeline`, shaders), which
  needs screenshots or visual verification, ideally on more than one backend
- public API changes that diverge from Bevy (see [Upstream compatibility](#upstream-compatibility))
- anything touching security, networking, or file system access

### Please don't

- Open large batches of unreviewed, agent-generated PRs. Keep it to a few open
  PRs at a time per contributor or agent, and make each one small and focused.
- Submit sweeping refactors without discussing them in an issue first. AI makes
  them cheap to write, but they stay expensive to review and to merge with
  upstream Bevy.
- Add code, assets, or text whose license or provenance you can't vouch for.

## Upstream compatibility

Titan is downstream of Bevy and merges each stable Bevy release (see
[MAINTAINING.md](MAINTAINING.md)). Every edit to upstream code makes those
merges harder, so:

- Prefer new crates, plugins, and tools over invasive edits to existing Bevy
  code.
- Keep crate names (`bevy_*`) and file layout as-is unless there's a strong
  reason to change them.
- If a change would make sense in Bevy, say so in the PR. We may propose it
  upstream, following
  [Bevy's contribution guide](https://bevy.org/learn/contribute/introduction)
  and [AI policy](https://bevy.org/learn/contribute/policies/ai/). That
  includes Bevy's rules about disclosure and human-written communication.

## Workflow

1. Fork `titan-engine/titan` and create a branch.
2. Make your change and run the checks in [AGENTS.md](AGENTS.md#checks).
3. Open a PR against `main` and fill in the template.

PRs are merged with a squash or a merge commit. Rebase merging is disabled
because upstream syncs need merge commits.

## Code of Conduct

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
