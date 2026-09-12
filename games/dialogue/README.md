# Dialogue tooling example

This small game-owned example demonstrates the reusable `titan-tools` library.
It defines one command, `check <file>`, without adding any dialogue knowledge to
Titan's launcher.

## Document format

A document contains one nonempty `speaker: text` record per line. The speaker
and text are trimmed for validation and must both be nonempty; a colon in the
text is allowed. Blank lines are malformed records. LF and CRLF line endings
are accepted. An empty document is invalid.

For example, `sample.dialogue` can contain:

```text
Guide: The path is open.
Player: Then we should keep moving.
```

## Run through Titan

From the repository root, build and launch the example through the generic
launcher:

```sh
cargo run --locked -- --project games/dialogue game --help
cargo run --locked -- --project games/dialogue game --json --help
cargo run --locked -- --project games/dialogue game check sample.dialogue
cargo run --locked -- --project games/dialogue game --json check sample.dialogue
```

The command parser reserves `--json` and `--help` as options. To check a file
whose name begins with either option, end option parsing explicitly:

```sh
cargo run --locked -- --project games/dialogue game check -- --json
```

Titan resolves the selected project and invokes its `titan-tools` binary as:

```text
cargo run --quiet --offline --manifest-path <project>/Cargo.toml \
  --bin titan-tools -- --titan-protocol 1 command <arguments>
```

The command list and argument descriptions in human help and JSON help come
from the same definitions. This example is CLI-only, so editor mode reports
that no editor is available; it does not start a JSON editor session.

The `titan-tools` entry point checks the protocol version before it looks up a
command or invokes a handler. An unsupported version is a tool invocation
failure on standard error, not a command JSON response.

## Responses and errors

Without `--json`, a successful check prints a game-owned human result such as
`valid dialogue: 2 record(s)` to standard output.

A successful JSON command prints exactly one JSON object on standard output and
exits with status `0`:

```json
{"schema_version":1,"ok":true,"result":{"records":2}}
```

The `check` result is an object with one game-defined field, `records`, the
number of valid records. A malformed document prints a versioned error object
to standard output and exits nonzero:

```json
{"schema_version":1,"ok":false,"error":{"code":"invalid_document","message":"line 1: expected a speaker: text record"}}
```

The example also uses `file_error` when the path cannot be read. Human mode
writes errors to standard error, for example:

```text
error [invalid_document]: line 1: expected a speaker: text record
```

A tool response exists only after Titan has successfully launched the project
binary. A missing project or manifest, unavailable Cargo executable, missing
`titan-tools` target, or Cargo build failure happens before the tool can emit a
response; those failures remain distinguishable by their nonzero status and
Cargo or launcher diagnostics on standard error. They are not converted into a
JSON envelope.
