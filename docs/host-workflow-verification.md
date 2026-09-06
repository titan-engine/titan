# Independent host workflow verification

Verified on macOS on 2026-09-05 by a fresh agent assigned only the documented
starter workflow. This verification did not change implementation files or
inspect engine internals. It read the Titan workflow skill,
`docs/verification.md`, `docs/cli.md`, `docs/browser.md`,
`starters/minimal/README.md` and `docs/host-tooling.md`, then checked the starter's
manifest, public host imports, build entrypoints and acceptance scripts.

## Executed check

From the Titan checkout:

```sh
/usr/bin/time -p python3 scripts/test-starter.py --browser
```

Exit status was 0. The script copied the starter outside the checkout to a
temporary `titan-starter-*/my-game` directory and applied the dependency-path
rewrite documented in the README. It used `target/starter-smoke` as its Cargo
target directory. The external copy was removed on normal completion.

The command successfully ran these checks in the copied package:

- `cargo fmt --all --check`
- `cargo test --all-targets`: three tests passed across library and native player.
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --bins`
- `cargo check --lib --target wasm32-unknown-unknown`
- `python3 scripts/build-browser.py`
- `node scripts/test-browser.mjs`: actual compiled WASM passed read-only policy,
  schema/target validation, input, stepping, PNG capture, validated fields and
  restart checks, including monotonic frame state and cleared queued input.
- `node --test web/inspector/bridge.test.mjs`: all four bridge tests passed.

The same command built the root CLI, started the copied native game with a
bounded lifetime and explicit mutation opt-in, and verified discovery, initial
paused status, capabilities, named entity and field inspection, injected input,
stepping, changed capture, restart restoring the initial capture, accepted and
rejected field writes, and a failure diagnostic bundle containing input history,
world state, API text and capture. Normal process termination exited successfully
and removed its discovery registration.

## Documentation and dependency assessment

The README and linked public documentation were sufficient for the verified
copy/configure/build/native-control/WASM workflow without reading engine
implementation. The acceptance script uses the README's path configuration;
the copied browser entrypoint resolved shared tooling through Cargo metadata.
The starter manifest and source import public Titan crates and local game code;
inspection found no `examples/support` or `procedural_rpg` imports. The automated
copy check also asserts that Rust sources do not reference `examples/support`.
No RPG support files were needed by the copied game.

No blocker was encountered. This check covers native headless control and actual
WASM execution under Node; it does not claim graphical canvas inspection, native
GPU presentation, macOS app-bundle verification, or root workspace quality gates.
Those require their separate checks.

## Observed cost

The aggregate command reported 9.00 seconds real, 17.39 seconds user and 3.78
seconds system time. This was a warm-cache run: Cargo dependencies and the WASM
target/toolchain were already available, and the shared starter target contained
prior build artifacts. Other verification work was running concurrently in the
checkout, so scheduling and shared-cache effects may affect the measurement.
This is one observed acceptance-run cost, not a cold-install benchmark or a
before/after iteration-speed claim.
