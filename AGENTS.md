# AGENTS.md

Instructions for coding agents working in this repository. Humans may find them
useful too.

## What this repository is

Titan is a downstream distribution of the [Bevy](https://bevy.org) game engine,
currently based on **Bevy 0.20.0**. The code is almost entirely Bevy. Crates
keep their `bevy_*` names on purpose. Don't rename crates, modules, or the
`bevy` identifiers in code.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before making changes. In particular:

- Keep changes small and focused, with one concern per PR.
- Prefer adding new crates, plugins, or tools over editing upstream Bevy code.
  Every edit to upstream code is a potential merge conflict on the next Bevy
  release.
- Fill in the PR template's **AI usage** section honestly.

## Layout

- `crates/`: the engine, one crate per subsystem (`bevy_ecs`, `bevy_render`, ...)
- `src/`: the top-level `bevy` crate, which re-exports the subsystems
- `examples/`: runnable examples, all registered in the root `Cargo.toml`
- `tests/`, `tests-integration/`: integration tests
- `tools/ci/`: the CI driver (`cargo run -p ci`)
- `_release-content/`: Bevy release notes and migration guides. Don't add
  Titan content here.

## Checks

Before opening a PR, run the CI tool. It mirrors `.github/workflows/ci.yml`:

```sh
cargo run -p ci -- format       # rustfmt
cargo run -p ci -- clippy       # clippy with the repo's lint config
cargo run -p ci -- test         # unit tests
cargo run -p ci -- doc-check    # docs build without warnings
cargo run -p ci                 # everything (slow)
```

For quick iteration, scope to the crate you're changing:

```sh
cargo check -p bevy_ecs
cargo test -p bevy_ecs
```

The minimum supported Rust version is set in the root `Cargo.toml`
(`rust-version`). Linux needs the system dependencies in
[docs/linux_dependencies.md](docs/linux_dependencies.md).

### Visual changes

The compiler can't verify rendering. For changes that affect what's on screen,
run a relevant example (`cargo run --example <name>`), capture before and after
screenshots, and attach them to the PR.

## Conventions

- Follow the existing style of the file you're editing. `rustfmt.toml` and
  `clippy.toml` are authoritative.
- Public items need doc comments. Changes to examples need an updated entry in
  the root `Cargo.toml` metadata and `examples/README.md`.
- `unsafe` blocks need a `// SAFETY:` comment explaining why the invariants hold.
