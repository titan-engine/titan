# Titan

Titan is a game engine being built in Rust for humans and AI agents to build games together. The goal is a visual editor for humans and a CLI that lets agents perform the same game-authoring tasks without automating the editor.

The CLI provides help and version information and can launch game-owned command and editor tooling; the repository also contains the reusable `titan-runtime` application lifecycle for Apple Silicon macOS. Rendering, a visual editor, and a playable sample game are not implemented yet. The first game will be a small, old-school 2D platformer, with macOS supported first, then Linux and Windows.

## Building on macOS

Install Rust 1.98.0 or newer using [rustup](https://rustup.rs/), and install Apple’s Command Line Tools:

```sh
xcode-select --install
```

The Command Line Tools provide the linker and macOS SDK used by the Rust toolchain. No separate graphics SDK or shader compiler is needed for the current CLI.

From the repository root, build and run:

```sh
cargo build --locked
cargo run --locked -- --help
cargo run --locked -- --version
```

## Running the native window example

The reusable `titan-runtime` crate currently provides the first native macOS
application and window lifecycle. It has no third-party Cargo dependencies
and uses handwritten AppKit bindings:

```sh
MACOSX_DEPLOYMENT_TARGET=11.0 cargo run --locked -p titan-runtime --example window
```

The example opens a responsive, resizable window and exits through normal
AppKit close or quit handling. See
[`docs/macos-window.md`](docs/macos-window.md) for the supported macOS
requirements, lifecycle, ownership, and ABI limitations.

You can also run the executable directly:

```sh
./target/debug/titan --help
```

Running `titan` without arguments shows help. Unsupported arguments print an error to stderr and return a nonzero exit status.

To check the minimum supported Rust version and current stable Rust:

```sh
rustup toolchain install 1.98.0 --profile minimal
rustup update stable
cargo +1.98.0 build --locked
cargo +stable build --locked
```

## Launching project tooling

A project can provide a Cargo binary target named `titan-tools`. Titan launches
that binary for game commands and editor actions without knowing the game's
commands or document formats:

```text
titan game [<game arguments>...]
titan edit [<editor arguments>...]
```

Once a project supplies a `titan-tools` target, you can launch it like this:

```sh
titan --project "./games/my game" game --help
titan --project "./games/my game" edit --help
```

Launching requires a `cargo` executable and a Rust toolchain. Because Titan
always uses Cargo's `--offline` mode, every dependency needed by the selected
project must already be available in Cargo's local cache; Titan does not fetch
dependencies for a tool launch.

Use `--project <directory>` before `game` or `edit` to select a project. Without
it, Titan selects its starting working directory. A relative project path is
resolved against that starting directory, before Titan changes the child
working directory. The child runs with the selected project directory as its
working directory, and Titan passes the selected project's absolute
`Cargo.toml` path to Cargo.

Titan invokes Cargo directly, without a shell, using this protocol:

```text
cargo run --quiet --offline --manifest-path <absolute-project>/Cargo.toml \
  --bin titan-tools -- --titan-protocol 1 command <game arguments>
cargo run --quiet --offline --manifest-path <absolute-project>/Cargo.toml \
  --bin titan-tools -- --titan-protocol 1 editor <editor arguments>
```

Every argument after `game` or `edit` is forwarded unchanged. Spaces,
option-like values, and non-UTF-8 argument bytes are not interpreted or
reconstructed by a shell. The tool inherits Titan's standard input, output,
and error streams. Cargo's build diagnostics remain on standard error, while
the tool's output and normal exit status are preserved. A missing project,
manifest, Cargo executable, or `titan-tools` binary target, as well as a build
failure, produces a nonzero result; Cargo's diagnostics are shown for Cargo
errors.

The ordinary `titan --help` and `titan --version` commands are handled by Titan
and do not inspect, build, or execute project code. The same is true when
`--project <directory>` precedes either host option; the project selection is
ignored for host help and version output. In contrast, `titan game --help` and
`titan edit --help` pass `--help` to `titan-tools`, so they compile and execute
project-provided code. Launching any project tool may also run Cargo build
scripts or other build hooks. Treat game-specific help and editor actions as
code execution, not as host help.

Tools run in the foreground. Titan does not install signal handlers, detach a
child, or keep a background service alive after the tool closes or is
interrupted.

## Getting involved

Start with the [development constraints](docs/development.md) to understand the project’s direction and requirements, then read the [contribution guide](CONTRIBUTING.md) for issue planning, branches, and pull requests. Work is planned in [GitHub issues](https://github.com/titan-engine/titan/issues) before implementation. Choose an issue with an agreed scope, and keep each pull request focused on one understandable change.

## License

Titan is dual-licensed under the [MIT License](LICENSE-MIT) and the [Apache License 2.0](LICENSE-APACHE). You may use it under either license.
