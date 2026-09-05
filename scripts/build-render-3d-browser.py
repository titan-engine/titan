#!/usr/bin/env python3
"""Build the opt-in browser GPU evidence page (no game host or event loop)."""
from pathlib import Path
import shutil
import acceptance_process as processes
from titan_build import cargo_metadata, package_by_name


def main():
    root = Path(__file__).resolve().parent.parent
    metadata = cargo_metadata(root)
    target = Path(metadata["target_directory"])
    version = package_by_name(metadata, "wasm-bindgen")["version"]
    tool_root = target / "titan/tools"
    bindgen = tool_root / "bin/wasm-bindgen"
    candidates = [bindgen]
    if installed := shutil.which("wasm-bindgen"):
        candidates.append(Path(installed))
    for candidate in candidates:
        if candidate.is_file() and processes.check_output(
            [str(candidate), "--version"], text=True, phase="build"
        ).strip() == f"wasm-bindgen {version}":
            bindgen = candidate
            break
    else:
        processes.run(["cargo", "install", "wasm-bindgen-cli", "--version", version,
                       "--locked", "--root", str(tool_root), "--force"],
                      cwd=root, check=True, phase="build")
    processes.run(["rustup", "target", "add", "wasm32-unknown-unknown"],
                  cwd=root, check=True, phase="build")
    processes.run(["cargo", "build", "--locked", "-p", "titan-render-wgpu",
                   "--example", "three_d_browser", "--target", "wasm32-unknown-unknown"],
                  cwd=root, check=True, phase="build")
    processes.run([str(bindgen), str(target / "wasm32-unknown-unknown/debug/examples/three_d_browser.wasm"),
                   "--target", "web", "--out-dir", str(root / "web/render-3d/pkg"),
                   "--out-name", "three_d_browser"], cwd=root, check=True, phase="build")
    print("Serve web/ on localhost and open /render-3d/?backend=webgpu or ?backend=webgl2.")


if __name__ == "__main__":
    main()
