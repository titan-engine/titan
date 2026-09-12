# Titan

Titan is a game engine being built in Rust for humans and AI agents to build games together. The goal is a visual editor for humans and a CLI that lets agents perform the same game-authoring tasks without automating the editor.

The project is just getting started. The CLI currently provides help and version information; there is no engine or editor yet. The first game will be a small, old-school 2D platformer, with macOS supported first, then Linux and Windows.

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

## Getting involved

Start with the [development constraints](docs/development.md) to understand the project’s direction and requirements, then read the [contribution guide](CONTRIBUTING.md) for issue planning, branches, and pull requests. Work is planned in [GitHub issues](https://github.com/titan-engine/titan/issues) before implementation. Choose an issue with an agreed scope, and keep each pull request focused on one understandable change.

## License

Titan is dual-licensed under the [MIT License](LICENSE-MIT) and the [Apache License 2.0](LICENSE-APACHE). You may use it under either license.
