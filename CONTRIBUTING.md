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
- **Proposed** means the approach or scope needs discussion. Contribute ideas and
  evidence there, and agree the implementation scope before starting.

For a typo or small documentation correction, a focused PR is welcome without
a separate issue. For larger changes, agree an issue with the maintainer first.
An issue's priority does not mean it is approved.

## Set up a development checkout

Install Git and Rust through rustup. The checkout selects Rust 1.98.1, rustfmt,
Clippy and the WASM target through `rust-toolchain.toml`. Native development and discovery support
macOS and Linux. A native window needs a graphics backend and desktop session;
headless tests do not require a GPU. Windows native discovery is not supported.
Use Python 3.12.3 for acceptance/build scripts and Node.js 22.23.2 for browser
acceptance tests (recorded in `.python-version` and `.node-version`).
Install these with your preferred version manager; ensure `python3` and `node`
on PATH report those versions. The browser build script installs matching `wasm-bindgen`
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
The [quality gates](docs/implementation-plan.md#constraints-and-quality-gates)
list additional native/browser and standalone-game checks by area. Ordinary
tests run without a GPU; GPU checks are opt-in. Say which checks you could not
run and why, so the maintainer can help cover them.

Preserve demo reference images and checksums unless an intentional visual change
is agreed. Never replace an expected checksum solely to make a test pass. Inspect
logs and captures before attaching them; omit credentials and raw runtime
discovery registrations.

## Update the verification environment intentionally

The pins are the routine verification baseline, not an MSRV declaration. We do
not set `rust-version`: older Rust releases have not been verified. The baseline
Rust 1.98.1 passed the workspace tests (including compiler-diagnostic fixtures),
strict Clippy, standalone/copied games, actual WASM and macOS packaging in
[main CI](https://github.com/titan-engine/titan/actions/runs/34044965687).
Python 3.12.3 and Node.js 22.23.2 were selected from that successful run's
[Ubuntu image inventory](https://github.com/actions/runner-images/blob/ubuntu24/20260831.293/images/ubuntu/Ubuntu2404-Readme.md).
CI explicitly installs all three versions on native, WASM, macOS and Pages
builds and prints their identities. Rust caches use `rustc -vV` plus platform
identity, so changing the compiler cannot restore another compiler's build cache.
The browser helper continues to install the lockfile-matched wasm-bindgen CLI.

Propose version changes in a focused PR. Update `rust-toolchain.toml` and the
starter's copy together, `.python-version`, `.node-version` and the prerequisite
versions above and in the starter README as applicable. Keep dependency updates
intentional and review the workspace and each independent game/fixture lockfile;
do not combine a toolchain pin change with unrelated dependency upgrades.
Run the quality gates for workspace tests, strict Clippy and diagnostic fixtures,
all standalone games, the copied starter, actual WASM and macOS bundles. Require
the existing CI and Browser demos builds to pass before adopting the pins.

To check newer stable compatibility without changing the routine baseline, use
one manually initiated comparison when proposing an update (no scheduled matrix):

```sh
rustup toolchain install stable --profile minimal --component rustfmt,clippy --target wasm32-unknown-unknown
rustc +stable --version
CARGO_BUILD_JOBS=4 cargo +stable test --workspace --all-targets
CARGO_BUILD_JOBS=4 cargo +stable clippy --workspace --all-targets --all-features -- -D warnings
```

Record the exact tested compiler and any diagnostic differences. These bounded
checks are a compatibility probe; adoption still requires the full existing CI
coverage with the candidate pinned. Update diagnostic expectations only for an
understood compiler change, preserving their intended error coverage. A copied
game keeps its own pin until its owner intentionally updates it.

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
