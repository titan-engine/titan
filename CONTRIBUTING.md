# Contributing to Titan

Titan is an experimental Rust game engine built around inspecting, controlling,
replaying, and verifying games through structured tools. Contributions with or
without AI tools are welcome: bug reports, documentation, examples, platform
testing, and focused engine improvements all help.

Olle Lukowski ([@olukowski](https://github.com/olukowski)) maintains Titan and
coordinates scope and reviews. Ask questions and discuss ideas in
[GitHub Discussions](https://github.com/titan-engine/titan/discussions).
Use [issues](https://github.com/titan-engine/titan/issues) for reproducible bugs
and agreed work. Treat people respectfully, explain technical disagreements,
and give actionable feedback.

## Find something to work on

- Browse [good first issues](https://github.com/titan-engine/titan/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22good%20first%20issue%22)
  for small tasks with starting points and verification steps.
- Browse [help wanted](https://github.com/titan-engine/titan/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22help%20wanted%22)
  or the public [development board](https://github.com/orgs/titan-engine/projects/1).
- Comment on an unassigned **Ready** issue before starting so the maintainer can
  coordinate ownership. Check that prerequisites are complete. You do not need
  project write access; the maintainer updates the board for you.
- **Proposed** means implementation is unapproved, even when an issue is fully
  specified. Reports may also need maintainer triage before becoming Ready.

For a typo or small documentation correction, a focused PR is welcome without
a separate issue. For larger changes, agree an issue with the maintainer first.
An issue's priority does not mean it is approved.

Keep brainstorming and planning in local conversations or GitHub Discussions;
do not commit plans, speculative roadmaps or task journals in any tracked format.
Once discussion yields concrete work, use an issue with an outcome,
acceptance/verification criteria, boundaries and approval state. Maintainers triage
prerequisites through native GitHub blocking relationships and use native
parent/sub-issue relationships for decomposition. Do not duplicate relationship
lists or titles in issue bodies; add prerequisite rationale only when useful beyond
those relationships.
The [planning and issue intake policy](docs/workflow.md#planning-and-issue-intake)
explains the handoff and what belongs in accepted repository documentation.

Bug reports are welcome without a proposed fix or completed work specification.
Share what happened and how to reproduce it as available; maintainers handle
triage and approval. For usage or contributor questions, use Discussions.

## Set up a development checkout

Install stable Rust with Cargo and Git. Native development and discovery support
macOS and Linux. A native window needs a graphics backend and desktop session;
headless tests do not require a GPU. Windows native discovery is not supported.
Python 3 is needed for acceptance/build scripts; Node.js is needed for browser
acceptance tests. The browser build script installs matching `wasm-bindgen`
tooling and adds the Rust WASM target when needed.

Fork [titan-engine/titan](https://github.com/titan-engine/titan), clone your fork,
and create a branch. Replace `YOUR-GITHUB-NAME` below:

```sh
git clone https://github.com/YOUR-GITHUB-NAME/titan.git
cd titan
git remote add upstream https://github.com/titan-engine/titan.git
git switch -c improve-example
cargo run --locked --example procedural_rpg
```

The headless demo finishes at tick 11 with three shards and an active shrine,
writing `target/titan/procedural-rpg.ppm` with RGBA checksum `f7a298f62ad75c1c`.
Run `cargo run --locked --example play_rpg` for the interactive version.
For your own game, follow the [standalone starter](starters/minimal/README.md).

You do not need Codex, a paid AI service, `gh`, a stack extension, or maintainer
access to contribute. An ordinary branch and GitHub PR are enough.

## Make and verify a change

Keep a PR focused on one outcome and update affected examples and docs. The
[docs index](docs/README.md) points to authoring and architecture guides. Engine
changes should follow the [vision](docs/vision.md) and relevant
[requirements](docs/design-requirements.md).

For Rust changes, run:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check -p titan -p titan-protocol -p titan-browser --target wasm32-unknown-unknown
```

The WASM check requires `rustup target add wasm32-unknown-unknown` if it is not
installed. For documentation-only changes, check local links, formatting, and
commands you changed. For tooling changes, run the relevant script tests.
The [quality gates](docs/verification.md#constraints-and-quality-gates)
list additional native/browser and standalone-game checks by area. Ordinary
tests run without a GPU; GPU checks are opt-in. Say which checks you could not
run and why, so the maintainer can help cover them.

Preserve demo reference images and checksums unless an intentional visual change
is agreed. Never replace an expected checksum solely to make a test pass. Inspect
logs and captures before attaching them; omit credentials and raw runtime
discovery registrations.

## Open a pull request

Push your branch to your fork and open a PR targeting Titan's `main`. Drafts are
welcome for early feedback. Describe the problem, resulting behavior, related
issue, and verification. Include screenshots for visual changes. Small,
coherent commits make the change easier to review.

The maintainer coordinates independent review and merges through the required
merge queue after checks and review pass. Outside contributors do not operate
the queue or need project permissions. Do not update a branch solely because
`main` moved; address actual conflicts or requested changes. If CI fails, inspect
the failing job and ask for help when the cause is unclear.

## Working with AI tools

AI assistance is optional. You remain responsible for understanding the change,
checking its behavior, and responding to review. Mention substantial generated
code or agent review in your PR so reviewers understand how it was verified.
Never present an agent review as a human approval. You do not need to publish
private prompts, session links, or local machine paths.

Maintainer-run agents follow [AGENTS.md](AGENTS.md) and the
[maintainer and agent workflow](docs/workflow.md), including attributed independent
agent review. The maintainer handles those operational steps for outside PRs;
they are not prerequisites for reporting a bug or submitting a contribution.

Titan is licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
Contribute material you have the right to share under those terms and retain
applicable third-party notices.
