# Implementation plan

[Milestone 2: build a second game from the starter](second-milestone.md) is
accepted. The user played both the native and browser arena versions and
confirmed that both work well. The procedural RPG milestone remains accepted,
including the sunlit-meadow result.

This file contains pending execution work; completed plans live in Git.

## Current objective: reduce duplicated host setup

The user selected this objective after accepting milestone 2. Audit RPG,
`starters/minimal`, and `games/arena`; extract only demonstrated reusable host
responsibilities into public APIs. Game rules, action mappings, commands,
validated fields, render extraction and presentation remain game-owned.

Implementation and local verification are complete:

- Public surface presenter, button-alias helper, browser session policy and PNG
  capture APIs replace duplicated mechanics without owning game behavior.
- Cargo-resolved build tooling keeps both standalone games externally copyable.
- Full native/WASM quality gates, hardware readback, all three native/browser
  players, relocated macOS bundles and fresh independent verification passed.
- Copied host setup fell from 1,066 to 739 lines per game. No build-speed gain is
  claimed. The audit records scope, reproducible counts and verification evidence.

Remote CI is the final gate for pushed increments; inspect the main commit's
GitHub Actions result before selecting further work. No additional engine
objective is inferred from this consolidation; future work needs a concrete
game requirement or the user's next priority.

No crate publication, visibility changes or new tags are authorized. Engine crate
versions remain 0.1.0. See [host setup audit](host-setup-audit.md) for boundaries
and evidence.

## Constraints and quality gates

Preserve the accepted RPG behavior and software checksum `190a92085def5677` for
changes unrelated to its visuals. Also preserve arena initial `1e5d05f547d53435`
and winning replay `be61b1c710b101b6`. Keep discovery authentication, browser control
opt-in, field validation, deterministic safe points, and bounded diagnostics.
Do not silently present transport timeouts as cancellation of running systems.

No speculative editor, 3D, networking, scene format, asset pipeline, parallel
executor, or broad reflection redesign is preapproved for the next objective. Use the fixed
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

Preserve CI coverage for the starter and both games. Run
native GPU readback and inspect the browser canvas when rendering changes.
Software images are exact references; GPU comparisons are integration evidence.
Commit small coherent increments, keep current examples compiling, and document
material API migrations alongside the affected usage guide.

Standalone and tooling gates:

```sh
python3 scripts/test-build-tools.py
python3 scripts/test-starter.py --browser
cargo fmt --manifest-path games/arena/Cargo.toml --all --check
cargo test --manifest-path games/arena/Cargo.toml --all-targets
cargo clippy --manifest-path games/arena/Cargo.toml --all-targets --all-features -- -D warnings
python3 games/arena/scripts/test-control.py
cargo check --manifest-path games/arena/Cargo.toml --lib --target wasm32-unknown-unknown
python3 games/arena/scripts/build-browser.py
node games/arena/scripts/test-browser.mjs
node --test games/arena/web/inspector/bridge.test.mjs
python3 scripts/test-macos-bundles.py # macOS
```
