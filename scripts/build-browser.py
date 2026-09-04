#!/usr/bin/env python3
"""Build matching wasm-bindgen web and Node packages for the RPG inspector."""
import json
from pathlib import Path
import subprocess

REPO = Path(__file__).resolve().parent.parent


def run(*arguments):
    subprocess.run(arguments, cwd=REPO, check=True)


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"], cwd=REPO, text=True
    ))
    target = Path(metadata["target_directory"])
    version = next(package["version"] for package in metadata["packages"] if package["name"] == "wasm-bindgen")
    tool_root = target / "titan/tools"
    bindgen = tool_root / "bin/wasm-bindgen"
    installed = subprocess.check_output([str(bindgen), "--version"], text=True).strip() if bindgen.exists() else ""
    if installed != f"wasm-bindgen {version}":
        run("cargo", "install", "wasm-bindgen-cli", "--version", version, "--locked", "--root", str(tool_root), "--force")
    run("rustup", "target", "add", "wasm32-unknown-unknown")
    run("cargo", "build", "-p", "titan-browser", "--target", "wasm32-unknown-unknown", "--release")
    wasm = target / "wasm32-unknown-unknown/release/titan_browser.wasm"
    for flavor, output in [("web", REPO / "web/inspector/pkg"), ("nodejs", target / "titan/browser-node")]:
        run(str(bindgen), str(wasm), "--target", flavor, "--out-dir", str(output))
    print("Browser packages built. Serve web/inspector over localhost HTTP to open the inspector.")


if __name__ == "__main__":
    main()
