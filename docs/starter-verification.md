# Independent starter verification

Verified on 2026-09-05 on macOS arm64 with Rust 1.98.1, Node.js 26.8.1,
and Python 3.9.6. This was a bounded starter validation, not the independent
arena-game exercise required by milestone 2.

The verifier read `starters/minimal/README.md`, [the historical milestone brief](https://github.com/titan-engine/titan/blob/5c211c04c9d5399a301cc3e6592d047d14b43664/docs/second-milestone.md),
the Titan workflow skill, and its implementation-plan, CLI, and browser guides.
No engine or game source inspection, RPG implementation/history, source edits,
or undocumented setup assistance was needed.

## Reproduction

From the Titan repository root, run the README's copy/configuration verbatim:

```sh
export TITAN_REPO="$PWD"
export GAME_DIR="$(mktemp -d)/my-game"
cp -R starters/minimal "$GAME_DIR"
python3 - <<'PY'
import json, os, re
from pathlib import Path
repo = Path(os.environ['TITAN_REPO']).resolve()
manifest = Path(os.environ['GAME_DIR']) / 'Cargo.toml'
manifest.write_text(re.sub(r'path = "(\.\./\.\.[^"]*)"',
    lambda m: 'path = ' + json.dumps(str((repo / 'starters/minimal' / m[1]).resolve())),
    manifest.read_text()))
PY
cd "$GAME_DIR"
export CARGO_TARGET_DIR="$TITAN_REPO/target/starter-smoke"
cargo test --all-targets
cargo run --bin titan-game
cargo run --bin play -- --frames 2
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --lib --target wasm32-unknown-unknown
python3 scripts/build-browser.py
node scripts/test-browser.mjs
node --test web/inspector/bridge.test.mjs
```

Every command passed. Four Rust tests and four browser bridge tests passed.
The headless run returned frame 0 and position `(80, 56)`. The native player
reported `rendered 2 GPU frames` and frame 2 at the same position. The browser
check reported `Starter actual-WASM policy, input, capture, fields and restart
checks passed.` Formatting, strict Clippy, and the WASM library check passed.

The copied package was outside the checkout at
`/var/folders/b7/glh0vdxd5ng65hshrdt98bpr0000gn/T/tmp.h3ixowuDC6/my-game`.
Its browser output remains in `web/inspector/pkg` for local inspection.
The WASM check executes generated bindings under Node. The parent subsequently
served this external copy on localhost:8083, opened `/play/`, clicked Play, and
verified the GPU canvas with the movable sprite and advancing frame counter.

The documented separate-process acceptance command also passed from the root:

```sh
cd "$TITAN_REPO"
CARGO_TARGET_DIR="$TITAN_REPO/target/starter-smoke" python3 scripts/test-starter.py --browser
```

It reported `Copied starter passed: build, discovery, input, step, capture,
restart, fields, diagnostics, shutdown.` Its additional copy passed actual-WASM
and bridge checks too. The script appends `starter-smoke` to the supplied target
directory, so this invocation used `target/starter-smoke/starter-smoke` for that
copy and rebuilt dependencies. This was harmless; it did not reuse the first
copy's compilation cache. Raw discovery registries and bearer tokens were not
read or retained in this record.

## Findings and scope

The documented setup was sufficient for this validation. No blocking defects
or documentation gaps were demonstrated. The build script's completion message
links to the web root; the README provides the useful `/play/` and `/inspector/`
entry points explicitly.

This verifies the unchanged starter's documented entry points. It does not
establish that a fresh agent can author the arena game from public guidance,
and does not substitute for playable browser review or milestone 2 acceptance.

## Integration review

The parent also rendered the starter in the browser and verified that pausing at
frame 288 and clicking Restart preserved frame 288. Review found and fixed an
initial browser-host dispatch bug: it rebuilt `App` rather than calling the
game's restart function. The shared player restart helper now has a native
regression test proving clock preservation and input clearing, and the rebuilt
WASM was checked on the canvas. [Browser canvas](milestone-2/starter-browser.png).

Root formatting, workspace tests, strict Clippy, WASM checks, native RPG control,
actual-WASM RPG control, bridge tests, and native GPU readback all passed. The
RPG software reference remains `190a92085def5677`; its GPU replay also passed
with `TITAN_GPU_TOLERANCE=0`.
