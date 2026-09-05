# Arena replay import responsiveness

[Issue #34](https://github.com/titan-engine/titan/issues/34) measures the existing
synchronous validation-before-install path at both import limits. This is an
investigation snapshot, not a performance guarantee or a new validation design.

## Reproduce

Use an otherwise idle machine and build optimized native and browser artifacts:

```sh
cargo build --release --manifest-path games/arena/Cargo.toml --bin titan-game --bin play --bin replay
cargo build --release -p titan-cli
python3 games/arena/scripts/build-browser.py
python3 docs/replay-import-responsiveness/native.py > native-output.json
python3 -m http.server 8080
```

Open `http://localhost:8080/games/arena/web/test/`, select **Replay import
responsiveness**, then use **Download import evidence**. The native probe uses
the real GPU player. Each probe uses seven samples and asserts that a recording rejected
only by the final snapshot/pixel check leaves the current session unchanged.

The 3,600-tick fixture is an idle-input recording produced through the actual
arena session. Its compact JSON is well below the byte limit, so the exact 2 MiB
case adds leading JSON whitespace. This isolates maximum accepted parsing size
without changing the parsed recording or file format. Native control uses the
compact recording because the CLI's separate argument-file limit is 1 MiB; the
native `replay` verifier measures the exact 2 MiB file.

## Results

Recorded on 2026-09-05 on an otherwise idle Apple M5 Pro with 64 GiB RAM,
macOS 27.0 (26A5425), arm64. Native used Rust 1.98.1 optimized binaries. Browser
used the optimized WASM build in Chrome TODO. The evidence revision and exact
per-sample values are recorded in [native-output.json](native-output.json) and
[browser-output.json](browser-output.json).

TODO: summarize results after both probes are recorded.

## Assessment

TODO: decide whether the measured interruption warrants incremental validation.

Any future design must keep the existing transactional boundary: fully validate
the complete recording before installation, cancel if the browser file-read
session changes, and compare the exact final snapshot and software pixels. It
must also define how work is canceled when a live session changes while batches
are pending. Seeking's 120-tick batching cannot simply be reused because seeking
operates on an already validated and installed recording.
