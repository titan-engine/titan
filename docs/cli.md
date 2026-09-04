# Titan CLI

The Titan CLI is an agent-friendly layer over ordinary Cargo workflows. Cargo
remains fully supported; the CLI adds predictable commands and structured
results.

During early workspace development, run it through Cargo:

```sh
cargo run -p titan-cli -- info
cargo run -p titan-cli -- check
cargo run -p titan-cli -- test
cargo run -p titan-cli -- run-example procedural_rpg
```

Every command accepts `--format human` (the default) or `--format json`:

```sh
cargo run -p titan-cli -- --format json check
```

In JSON mode, stdout contains exactly one JSON result. Cargo output is captured
inside that result so agents do not need to combine several output streams.
Invocation or CLI failures use a nonzero process exit code.

## Native headless control

Native discovery currently supports macOS and Linux. Start the RPG in a terminal:

```sh
cargo run --example procedural_rpg -- --serve --instance demo
```

It starts paused at frame 0. In another terminal, from the same project:

```sh
cargo run -p titan-cli -- --format json instances
cargo run -p titan-cli -- --format json --instance demo capabilities
cargo run -p titan-cli -- --format json --instance demo entities --name player
cargo run -p titan-cli -- --format json --instance demo commands
cargo run -p titan-cli -- --format json --instance demo input 1 --actions '{"right":{"kind":"button","value":true}}'
cargo run -p titan-cli -- --format json --instance demo step 1
cargo run -p titan-cli -- --format json --instance demo invoke spawn_shard --arguments '{"x":0,"y":0}'
cargo run -p titan-cli -- --format json --instance demo capture
```

Each command discovers and attaches to the selected runtime. `--instance` can
be omitted when exactly one runtime matches. Multiple matches produce an
explicit ambiguity error. `--project DIR` selects a project (the current
directory by default); it also selects the directory for Cargo workflow
commands. `--timeout-ms N` bounds an inspection request and defaults to 5000.

`status` reports the clock and run mode. `entities` supports `--name`, repeated
`--component`, `--cursor`, and `--limit`. Use `entity INDEX GENERATION` to inspect
one returned entity ID. Component values are currently opaque, but type names
and the RPG's `ActiveShrine` marker expose semantic state.

Input is a complete snapshot for one future fixed tick. The RPG accepts button
values for `up`, `down`, `left`, and `right`; unspecified frames release all
actions once injection is active. Submitting the same future frame replaces
its previous snapshot. Queue several frames before stepping to replay a route.

Runtime operations return protocol response envelopes directly in JSON mode.
Discovery returns `{ "status": "success", "instances": [...] }`, with tokens
removed. Local parsing, discovery, or transport failures return a structured
`status: "failure"` object and nonzero exit code. Cargo workflow commands retain
the `CommandResult` format described above. Help and version requests remain
ordinary CLI text.

Captures return an absolute artifact path, dimensions, format, and checksum.
The RPG writes a PPM file under `target/titan/<instance>-<pid>/capture.ppm` in the
selected project. A subsequent capture replaces that file.

Ctrl-C or SIGTERM stops the game and removes its registration. For bounded
runs, add `--run-for-ms 30000` to the example. Per-instance registration files
are owner-only and live under `target/titan/instances`; each run has an
ephemeral loopback endpoint and random bearer token. Discovery ignores invalid,
insecure, or stale registrations. Transport workers queue requests and never
access the ECS world. The game drains the bounded queue between schedules.
Browser-origin requests are rejected by this native adapter.

Expired queued requests are skipped. A timeout after a request has started
does not roll back effects; inspect state before retrying a mutation.

Run the complete separate-process acceptance check with Python 3:

```sh
python3 scripts/test-control-loop.py
```

It builds the example and CLI, drives the reference route, checks shrine
activation and exact image checksum, invokes a command, checks structured
failures, and verifies clean shutdown.

