#!/usr/bin/env python3
"""Build this copied game's WebAssembly and matching browser/Node bindings."""
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def run(*arguments):
    subprocess.run(arguments, cwd=ROOT, check=True)


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"],
        cwd=ROOT, text=True,
    ))
    target = Path(metadata["target_directory"])
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "wasm-bindgen")
    tool_root = target / "titan/tools"
    candidates = [tool_root / "bin/wasm-bindgen"]
    # A path dependency's checkout may already contain the matching CLI. This is
    # only a cache optimization: a separate copied starter installs its own tool.
    for package in metadata["packages"]:
        if package["name"] == "titan":
            candidates.append(Path(package["manifest_path"]).parent / "target/titan/tools/bin/wasm-bindgen")
    if shutil.which("wasm-bindgen"):
        candidates.append(Path(shutil.which("wasm-bindgen")))
    bindgen = None
    for candidate in candidates:
        if candidate.is_file():
            result = subprocess.run([str(candidate), "--version"], capture_output=True, text=True)
            if result.returncode == 0 and result.stdout.strip() == f"wasm-bindgen {version}":
                bindgen = candidate
                break
    if bindgen is None:
        run("cargo", "install", "wasm-bindgen-cli", "--version", version, "--locked", "--root", str(tool_root), "--force")
        bindgen = tool_root / "bin/wasm-bindgen"
    run("rustup", "target", "add", "wasm32-unknown-unknown")
    run("cargo", "build", "--lib", "--target", "wasm32-unknown-unknown", "--release")
    package = next(p for p in metadata["packages"] if p["id"] == metadata["resolve"]["root"])
    library = next(t for t in package["targets"] if "cdylib" in t["crate_types"])
    wasm = target / "wasm32-unknown-unknown/release" / (library["name"].replace("-", "_") + ".wasm")
    for flavor, output in [("web", ROOT / "web/inspector/pkg"), ("nodejs", target / "titan/browser-node")]:
        run(str(bindgen), str(wasm), "--target", flavor, "--out-dir", str(output), "--out-name", "titan_game")
    print("Built browser packages. Run python3 -m http.server --directory web 8080, then open http://localhost:8080/.")


if __name__ == "__main__":
    main()
