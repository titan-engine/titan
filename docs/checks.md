# Checks

The GitHub Actions workflow in `.github/workflows/checks.yml` runs five independent checks on `macos-latest` for both pull requests and merge-queue candidates:

- `macOS / Format` checks rustfmt output.
- `macOS / Clippy` runs Clippy with warnings denied.
- `macOS / Stable` builds and tests the workspace with current stable Rust.
- `macOS / MSRV` builds and tests the workspace with the Rust version declared by the workspace metadata.
- `macOS / Dependency policy` allows dependencies between workspace crates and rejects third-party dependencies.

The workflow uses no repository secrets and grants the checkout only read access to repository contents. The checkout and Python setup actions use explicit release tags. Both jobs that use Python explicitly install Python 3.11.

## Requirements

Run these commands from the repository root on macOS. Install Apple's Command Line Tools for the linker and macOS SDK, then install [rustup](https://rustup.rs/). The local dependency check also requires Python 3.11 or newer, which provides the standard-library `tomllib` module. Check `python3 --version` before running it; Apple's bundled Python may be older. No Python packages or Cargo dependencies are required.

## Local commands

Install the stable toolchain and the components used by the checks:

```sh
rustup toolchain install stable --profile minimal --no-self-update
rustup component add rustfmt clippy --toolchain stable
```

Check formatting:

```sh
cargo +stable fmt --all -- --check
```

Run Clippy with warnings denied:

```sh
cargo +stable clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Build and test with current stable Rust:

```sh
cargo +stable build --workspace --locked
cargo +stable test --workspace --locked
```

Read the MSRV from Cargo's workspace metadata, install that toolchain, and build and test with it. A version such as `1.98` means `1.98.0` here, not the latest patch release in that series:

```sh
msrv="$(
  cargo +stable metadata --no-deps --format-version 1 |
    python3 -c '
import json
import sys

versions = {
    package["rust_version"]
    for package in json.load(sys.stdin)["packages"]
    if package.get("rust_version")
}
if len(versions) != 1:
    raise SystemExit(
        f"expected one workspace rust-version, found {sorted(versions)!r}"
    )
version = next(iter(versions))
print(version if version.count(".") == 2 else f"{version}.0")
'
)"
rustup toolchain install "$msrv" --profile minimal --no-self-update
cargo +"$msrv" build --workspace --locked
cargo +"$msrv" test --workspace --locked
```

Check that all dependencies stay within the workspace, including development, build, optional, target-specific, and inherited workspace dependencies:

```sh
python3 scripts/check-dependencies.py
python3 scripts/test_dependency_policy.py
```

The check uses offline Cargo metadata to identify workspace members and their dependencies, including automatically included path dependencies. It also reads manifests to check unused `[workspace.dependencies]` entries and reject source overrides. It does not rely on `Cargo.lock` or download dependencies.

Direct path dependencies and `workspace = true` are allowed when they resolve to another crate inside the workspace. Registry and Git dependencies, paths outside the workspace, paths to excluded crates, and `[patch]` or `[replace]` overrides are rejected. A version requirement alongside an internal path is allowed.

The regression tests create temporary Cargo workspaces. They check internal paths, inherited and renamed dependencies, optional and non-host third-party declarations, excluded and external paths, and third-party dependencies hidden in a member crate. The temporary workspaces are removed after the tests.
