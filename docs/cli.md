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
cargo run -p titan-cli -- compare-images expected.png actual.png --output target/visual-diffs --exact
```

Every command accepts `--format human` (the default) or `--format json`:

```sh
cargo run -p titan-cli -- --format json check
```

In JSON mode, stdout contains exactly one JSON result. Cargo output is captured
inside that result so agents do not need to combine several output streams.
Invocation or CLI failures use a nonzero process exit code.

## Offline image comparison

`compare-images` compares two existing PNG files without starting or attaching to
a game. Exact mode requires identical decoded RGBA bytes:

```sh
target/debug/titan compare-images expected.png actual.png \
  --output target/visual-diffs --exact
```

Without `--exact`, the existing perceptual defaults apply: block SSIM must be at
least `0.99`, linear-RGB RMSE must be at most `0.01`, and no maximum individual
RGBA-byte error is imposed. Override any tolerance explicitly as needed:

```sh
target/debug/titan compare-images expected.png actual.png \
  --output target/visual-diffs \
  --minimum-ssim 0.995 \
  --maximum-linear-rmse 0.005 \
  --maximum-channel-error 4
```

`--exact` conflicts with tolerance flags so an invocation cannot imply two
comparison policies. The inputs are decoded with Titan's normal bounded PNG
limits: each must be a regular file of at most 8 MiB, dimensions are limited to
4096×4096, decoded RGBA data to 64 MiB, and decoder allocation accounting to
160 MiB. Malformed, animated, truncated, unsupported, oversized, and unequal-size
inputs are rejected before a report is written.
Input and output paths must be representable as UTF-8 so the same usable paths
can be returned in structured JSON on every supported platform.

The output argument names a report root, not a file to overwrite. Each completed
comparison creates a unique owner-only directory below it containing lossless
`expected.png` and `actual.png` copies, `difference.png`, and `report.json`.
Artifacts and their decoded inputs are bounded to 64 MiB apiece. A failed write
cleans up only its newly created directory and never replaces an existing report.
The [diagnostics crate guide](../crates/titan-diagnostics/README.md#offline-comparison-reports)
documents the difference-image channels and metric definitions.

Human output identifies `PASS` or `MISMATCH`, summarizes every metric and prints
the manifest and difference-image paths. JSON mode emits the normal local
`CommandResult` with `command: "compare_images"` and `data.type:
"image_comparison"`. `data` contains the dimensions, selected options, unchanged
comparison metrics, input paths, and absolute paths for the report directory,
manifest, source copies, and difference image. Completed mismatches retain this
data and the report while setting `success: false`, `exit_code: 2`, and
`error_code: "visual_mismatch"`.

The process exits with status 0 when thresholds pass, 2 when comparison completes
but thresholds do not pass, and 1 for invalid input or execution failure. Invalid
files, dimensions, or thresholds use `error_code: "invalid_value"`; report-write
failures use `error_code: "artifact_write_failed"`. Those failures have no
comparison `data`. Under the default diagnostic policy a mismatch or failure also
writes the usual CLI diagnostic bundle beneath `--project`; use `--diagnostics
never` when only the comparison report is desired.

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
one returned entity ID. Explicitly registered component fields expose typed
values and field metadata; other components remain opaque. The RPG's
`ActiveShrine` marker also exposes semantic state.

Use `set-field INDEX GENERATION COMPONENT FIELD --value JSON` to change a
registered writable field. Use the entity ID, component name, and field name
returned by inspection. For example, after starting the RPG with
`--serve --allow-mutation`, copy a Position component name from `entity` and run:

```sh
cargo run -p titan-cli -- set-field 0 0 'COMPONENT_NAME_FROM_INSPECTION' x --value -3.5
```

Replace the example entity index/generation with the inspected ID. `--value`
accepts any valid JSON value; strings need JSON quotes (for example,
`--value '"hello"'`) and booleans use `--value true`. The runtime validates the
registered field type, entity generation, field permissions, and mutation
policy. Malformed JSON fails locally before discovery. Mutation is disabled by
default in the RPG and must be explicitly enabled with `--allow-mutation`.
Unregistered fields cannot be changed through this command.

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

## Diagnostics and controlled budgets

`--diagnostics on-failure` is the default. Successfully parsed commands write a
unique bundle under the selected project's `target/titan/diagnostics` when
local validation, discovery, transport, a runtime request, or Cargo fails.
`--diagnostics always` also captures successful commands; `--diagnostics never`
disables CLI capture. Argument syntax errors before project/policy resolution
remain structured errors without artifacts.

Inspection failure paths appear in `error.details.diagnostic_bundle`. Existing
runtime-provided paths are preserved without writing a duplicate CLI bundle.
Cargo results and successful CLI captures use top-level `diagnostic_bundle`.
The path points to `bundle.json`; human Cargo output also prints it. Artifact
write failures add `diagnostic_error` and preserve the original command outcome.
CLI policy does not override a running game's independently configured policy.

CLI fallback bundles preserve local error messages or the received protocol
response (including its observed frame, revision, and error details);
Cargo bundles include elapsed timing and at most 1 MiB of retained output per
stream, with a truncation marker when needed. They exclude environment variables,
registry authentication tokens, and separately recorded invocation arguments.
Error messages, response data, and child output may
contain application-emitted data, so review it before sharing. Runtime bundles
can contain richer world state, history, and captures; the CLI does not attempt
additional inspection after transport failure.

`step N --max-frames M` rejects N greater than M locally with `budget_exceeded`;
M defaults to 10000. This is a per-request bound, not a cumulative run budget.
`--timeout-ms` still bounds the remote request independently.

`check`, `test`, and `run-example` have a wall-clock budget of 120000 ms,
including compilation. Override it with `--process-timeout-ms N`. A timeout
returns nonzero, `error_code: "timeout"`, and a diagnostic bundle under the
default policy. Ordinary process failure uses `process_failed`; inability to
start Cargo uses `spawn_failed`. Retained stdout and stderr are bounded even
for noisy children. On macOS/Linux the CLI kills the Cargo process group on
completion or timeout, including ordinary test/example descendants. Processes
that deliberately escape the group are outside this guarantee. On other
platforms only the direct child is terminated. Pipe draining has an additional
100 ms grace period and never waits indefinitely for descendant-held pipes.

## Read-only game queries

`queries` discovers game-defined reads and their argument metadata. `query`
executes one against the selected runtime without advancing its clock:

```sh
target/debug/titan --format json --project games/arena --instance arena-live queries
target/debug/titan --format json --project games/arena --instance arena-live query arena_state
target/debug/titan --format json --project games/arena --instance arena-live query recording > arena-recording.json
```

Optional `--arguments '{"key":"value"}'` supplies a JSON object. Query results
retain the normal response envelope, including observed frame and revision.
Read-only attachment permits queries; command invocation and other controls
still require the live host's explicit opt-in. See the arena README for starting
an actual native player with inspection and verifying its exported recording.

Both `query` and `invoke` also accept `--arguments-file PATH`, mutually exclusive
with inline `--arguments`. The path is relative to the CLI's current directory,
not `--project`. The file must contain a UTF-8 JSON object and be a regular file
of at most 1 MiB; the runtime may enforce smaller game-specific limits. File and
JSON errors are reported before runtime discovery. This supports larger payloads
such as saved game state without shell interpolation:

```sh
target/debug/titan --format json --project games/arena --instance arena-live invoke load_save --arguments-file arena-load.json
```

`arena-load.json` contains the command argument object (`{"save": ...}`), rather
than a CLI response envelope. Query responses still include their envelope;
extract the returned value explicitly when preparing a later command.
