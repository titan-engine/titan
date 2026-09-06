# Arena snapshots

Arena now exercises [the save/load boundary](save-load.md) with a game-owned JSON
format. This work is on `main`; the published `v0.3.0` source tag precedes this
increment.

## Use the browser player

Build the arena browser package and open its Play page. **Save snapshot** exports
the current game state as `arena-save.json`, including while playing with
read-only inspection. To restore it, pause the game, enable inspection controls,
then choose the file using **Load snapshot**. Files stay local to the browser.

Imports are limited to 64 KiB. The page checks the file size before reading and
the UTF-8 size afterward. If the player, reset epoch, pause state or control
permission changes during the asynchronous file read, the import is rejected.
Rust then validates the save before any gameplay changes are installed.

## Use native inspection

From the repository root, build the CLI and start an inspectable player:

```sh
cargo build -p titan-cli
cargo run --manifest-path games/arena/Cargo.toml --bin play -- --inspect --allow-control --project games/arena --instance arena-save
```

In another terminal, export the read-only query and prepare a command argument
file. The CLI response envelope is distinct from the raw save document:

```sh
target/debug/titan --format json --project games/arena --instance arena-save query save > arena-save-response.json
python3 -c 'import json,sys; json.dump({"save":json.load(sys.stdin)["response"]["value"]},sys.stdout)' < arena-save-response.json > arena-load.json
target/debug/titan --format json --project games/arena --instance arena-save invoke pause
target/debug/titan --format json --project games/arena --instance arena-save invoke load_save --arguments-file arena-load.json
```

To load the browser's raw export instead, wrap that file with `{"save": ...}`.
`--arguments-file` avoids embedding the payload in a shell argument; it is shared
by all CLI queries and commands. Its 1 MiB transport bound does not override
the game's 64 KiB limit.

The protocol operations are query `save` with empty arguments and command
`load_save` with `{"save": <snapshot>}`. Live native/browser loading requires both
pause and control permission. The standalone controlled headless server retains
its existing command policy and executes loading at an exclusive safe point.

## State and failure behavior

Format version 1 contains the game seed, player position, all 14 enemy-pool slots,
and the complete `Run`: elapsed ticks, health, outcome, spawn progress, current
RNG state, contact cooldown, dash duration/cooldown, facing and locked direction.
Pool slots express game ordering; runtime entity IDs are not serialized.

Unknown fields, unsupported versions/seeds, missing slots, invalid positions and
inconsistent gameplay counters are rejected. Candidate data and the initialized
target's required components/resources are checked before assignments begin.
This is validation for the current arena rules, not a compatibility promise for
future game revisions. See [an exported example](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/save-load/browser-save.json).

A successful load reuses the initialized game's entity and asset handles, installs
the gameplay state, recomputes HUD text and refreshes extraction. It clears held
and buffered input, scheduled input and both pointer gesture sources. The live
session resets wall-clock accumulation while preserving its pause state,
monotonic host frame and inspection identity. Successful protocol loading is
accounted for as a state revision; rejected loads leave state and revision alone.

A successful load starts a valid new recording whose initial snapshot is the
restored gameplay state. Restart also starts a new segment. These recordings can
be [played interactively or verified headlessly](arena-replay.md). Loading a save
is blocked during playback; exit playback first. General world serialization
remains outside this game-owned format.

## Verification

Game tests restore mid-dash and contact-cooldown snapshots and compare complete
exports and pixels after identical input. Session and actual-WASM tests cover
permission, pause, stale-input cancellation and failed-load nonmutation. See the
[game guide](../games/arena/README.md) for current commands and
[replay guide](arena-replay.md) for snapshot recording origins.

The [historical save/load report](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/arena-save-load.md#verification)
records the original file-chooser exercise, which predates snapshot-backed
recording support. Save representation and installation policy remain game-owned.
