# Inspection failure regression cases

## Run maintained regression cases

These maintained cases support [issue #96](https://github.com/titan-engine/titan/issues/96).
They generate their own runtime fixtures; historical JSON output is not input.
Run from the repository root on macOS or Linux:

```sh
cargo build --locked -p titan-cli -p titan --example procedural_rpg --bin titan
python3 scripts/inspection-repair/native.py --output target/evidence/inspection-repair/native-output.json
python3 scripts/build-browser.py
node scripts/inspection-repair/browser.mjs --output target/evidence/inspection-repair/browser-output.json
```

The native probe starts two owned, bounded RPG processes in a temporary project,
uses the default diagnostic policy, reads selected bundle fields and `api.txt`,
and terminates its processes and checks registration cleanup. It never reads
raw discovery registrations. It edits no fields successfully; it resumes,
pauses, and steps its own fixture. Python assertions must remain enabled.
The browser probe runs actual compiled WASM under Node, including an explicitly
controlled fixture. It does not drive a DOM, message bridge, window, or GPU.
Neither probe is a new CI gate or a general repair API.

## Historical investigation

[Issue #39](https://github.com/titan-engine/titan/issues/39) observed gaps in
clock-recovery reads, host permission guidance and explicit native target selection.
The qualitative output-only assessment supplied investigator-selected follow-up
reads; it was not an unaided-agent success-rate measurement.

The [original report, transcripts and probes](https://github.com/titan-engine/titan/blob/57f12a8f95dad3e7819b43f9ab2708fdbe10d708/docs/inspection-repair/README.md)
are preserved at evidence revision `57f12a8f95dad3e7819b43f9ab2708fdbe10d708`.
They measured engine source `5736101cbc6462b3bb51ce7617f8300605175b9a` on
2026-09-05, macOS arm64, Rust 1.98.1, Node 26.8.1 and Python 3.9.6.
Use that report for original commands and inputs in a disposable checkout; it
does not establish fresh behavior or an implemented repair contract.
Maintained reruns default to the ignored output paths above and accept `--output`;
choose an ignored path for alternatives. Current host policy is documented in
[CLI control](../cli.md#native-headless-control) and [browser inspection](../browser.md).
