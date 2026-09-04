# Implementation plan

The active objective is [milestone 2: build a second game from the starter](second-milestone.md).
The procedural RPG milestone is accepted, including the sunlit-meadow result.
This file contains pending execution work; completed plans live in Git.

The starter boundary is implemented in `starters/minimal`; see its README for
copy/build instructions and [the audit](starter-audit.md) for the boundary.
`scripts/test-starter.py --browser` verifies an external copy through native and
actual-WASM control loops. The RPG checksum remains `190a92085def5677`.

## Build the arena game with an independent agent

Give a fresh agent the milestone's game brief, starter, and repository-local
documentation, without the RPG implementation history or advance explanations
of anticipated gaps. Let it inspect public engine source when necessary, but
record when documentation was insufficient. The game must use its own components,
resources, systems, input definitions, and generated assets.

Record concrete obstacles and the commands, failures, and artifacts that exposed
them. Distinguish game bugs, missing documentation, reusable host setup, and
engine limitations. A successful build achieved through undocumented assistance
is not sufficient evidence that the starter is usable.

Completion: the arena game can be played on native and browser targets and driven
headlessly through the same inspection protocol, with deterministic replay and
state assertions.

## Fix demonstrated gaps and repeat the exercise

Prioritize issues that prevented or confused the independent build. Keep gameplay
rules in the game; move a helper into the engine only when a reusable responsibility
is clear. Collisions, optional queries, richer metadata, and camera support are
candidates, not a preapproved feature backlog.

Correct the relevant APIs or docs, then have a fresh agent verify the corrected
workflow. Add regression coverage for demonstrated engine defects. Preserve both
games and their independent replay expectations.

Completion: the final verification succeeds with documented steps and no private
RPG dependencies. Record the remaining limitations honestly, with no unresolved
blockers to the milestone's acceptance criteria.

## Close the milestone

Capture native/browser play evidence, headless replay results, one useful failure
bundle, and an agent iteration that diagnoses and fixes a failed attempt. Link
that evidence from the milestone document, request user review of the playable
result, and choose the next objective from the observed gaps. Remove completed
execution sections from this plan instead of accumulating historical checklists.

## Constraints and quality gates

Preserve the accepted RPG behavior and software checksum `190a92085def5677` for
changes unrelated to its visuals. Keep discovery authentication, browser control
opt-in, field validation, deterministic safe points, and bounded diagnostics.
Do not silently present transport timeouts as cancellation of running systems.

No speculative editor, 3D, networking, scene format, asset pipeline, parallel
executor, or broad reflection redesign is part of this milestone. Use the fixed
arena view unless the game demonstrates a camera requirement. Keep current
platform limitations documented rather than expanding platform scope.

Each implementation increment must pass:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check -p titan -p titan-protocol -p titan-browser --target wasm32-unknown-unknown
```

For shared host, protocol, input, or game changes, also run the existing native
and actual-WASM control loops:

```sh
python3 scripts/test-control-loop.py
python3 scripts/build-browser.py
node scripts/test-browser.mjs
node --test web/inspector/bridge.test.mjs
```

Extend CI to build and test the starter and arena targets when introduced. Run
native GPU readback and inspect the browser canvas when rendering changes.
Software images are exact references; GPU comparisons are integration evidence.
Commit small coherent increments, keep current examples compiling, and document
material API migrations alongside the affected usage guide.
