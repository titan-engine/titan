# Implementation plan

The active objective is [milestone 2: build a second game from the starter](second-milestone.md).
The procedural RPG milestone is accepted, including the sunlit-meadow result.
This file contains pending execution work; completed plans live in Git.

The starter boundary is implemented in `starters/minimal`; see its README for
copy/build instructions and [the audit](starter-audit.md) for the boundary.
`scripts/test-starter.py --browser` verifies an external copy through native and
actual-WASM control loops. The RPG checksum remains `190a92085def5677`.

## Pending: playable user review

The independent arena build, diagnosed failure/fix and fresh verification are
recorded in [the exercise](arena-exercise.md) and [verification](arena-verification.md).
Native and browser graphics, exact replay, bounded diagnostics and CI coverage
are in place. Review the [playable arena](../games/arena/README.md) with the user.
If tuning or presentation changes are requested, retain semantic assertions and
review before changing exact image expectations.

After acceptance, choose the next objective from demonstrated costs: duplicated
host adapters across standalone games are a candidate for small consolidation.
No engine feature is required to complete this milestone. Keep current platform
and separate player/inspection-instance limitations documented. Do not mark user
review complete or choose a new implementation objective on the user's behalf.

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
