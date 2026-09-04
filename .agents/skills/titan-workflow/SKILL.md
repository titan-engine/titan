---
name: titan-workflow
description: Build, run, inspect, replay, and diagnose Titan games using this repository's CLI and deterministic RPG example. Use for game iteration and engine changes that need runtime evidence.
---

# Titan game iteration

Run commands from the repository root. Read `docs/implementation-plan.md` for
current priorities and `docs/cli.md` for flags and structured output. Use
`docs/ecs-authoring.md` when editing systems, `docs/browser.md` for WASM, and
`docs/rendering.md` for interactive or GPU work. These paths are relative to the
repository root, three directories above this skill folder.

Build the native control tools with:

```sh
cargo build -p titan-cli -p titan --bin titan --example procedural_rpg
```

Launch a bounded paused game in a separate process:

```sh
target/debug/examples/procedural_rpg --serve --instance iteration --run-for-ms 120000
```

Use `target/debug/titan --format json --instance iteration` followed by
`capabilities`, `status`, `entities`, or `commands`. Capabilities and command
metadata describe what this runtime supports. Multiple matching instances require
an explicit selection. Discovery files contain bearer tokens: use CLI instance
output for reports and do not copy the raw registry into artifacts.

Inputs are complete snapshots for a future fixed frame, not incremental changes:

```sh
target/debug/titan --format json --instance iteration input 1 --actions '{"right":{"kind":"button","value":true}}'
target/debug/titan --format json --instance iteration step 1
target/debug/titan --format json --instance iteration capture
```

Read `observed_frame`, `state_revision`, and the structured outcome. Failed
operations may have partially changed state; unchanged revision means no
successful operation was recorded, not transaction rollback. Do not blindly
retry a timed-out mutation: inspect first because it may already have applied.
On failure, read `error.details.diagnostic_bundle` when present. Its manifest,
`api.txt`, and optional `capture.png` provide local evidence; inspect logged
capture/write failures rather than assuming every artifact exists.

`python3 scripts/test-control-loop.py` exercises discovery, exact replay,
inspection, commands, captures, diagnostic failures, and shutdown in separate
processes. The reference route is right 2, down 3, right 6: eleven fixed ticks,
three collected shards, active shrine, software RGBA checksum
`98618cd721c5b52d`. Preserve it for behavior-neutral engine changes. For intentional
visual changes, compare before/after images and semantic results before updating
an expected checksum; a new checksum alone is not evidence of improvement.

After an engine change, use the quality gates listed in the implementation plan.
Ordinary `cargo test` remains useful; the Titan CLI adds structured results and
bounded execution. Stop owned runtime processes after verification and let their
normal shutdown remove discovery registrations.
