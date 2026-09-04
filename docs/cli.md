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

Runtime discovery, attachment, inspection, controlled stepping, and capture
will be added on top of the transport-neutral `titan-protocol` contract.

