"""Public build helpers for independent Titan packages.

Call browser(root, metadata, package_name=..., out_name=...) or
macos_app(root, metadata, argv=None). Obtain metadata with cargo_metadata(root).
Game entrypoints own targets, output names, and application identity. These
helpers do not import any game code. See docs/host-tooling.md.
"""
import acceptance_process as processes
import argparse
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile


def cargo_metadata(root):
    return json.loads(processes.check_output(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"],
        cwd=root, text=True, phase="build",
    ))


def package_by_name(metadata, name):
    matches = [p for p in metadata["packages"] if p["name"] == name]
    if len(matches) != 1:
        raise ValueError(f"expected one resolved {name!r} package, found {len(matches)}")
    return matches[0]


def asset_source(root, assets_source):
    """Resolve optional game resources. An explicitly selected source must exist."""
    source = Path(assets_source) if assets_source is not None else Path(root) / "assets"
    if not source.is_absolute():
        source = Path(root) / source
    if not source.exists() and assets_source is None:
        return None
    if not source.is_dir():
        raise ValueError(f"asset source is not a directory: {source}")
    # Resources are self-contained files; do not silently package external links.
    for entry in source.rglob("*"):
        if entry.is_symlink() or not (entry.is_file() or entry.is_dir()):
            raise ValueError(f"asset source contains a non-regular entry: {entry}")
    return source.resolve()


def copy_assets(source, destination):
    """Replace a generated resource directory, avoiding files from earlier builds."""
    if source is None:
        return
    destination = Path(destination)
    if (source == destination.resolve() or source in destination.resolve().parents
            or destination.resolve() in source.parents):
        raise ValueError("asset output must be outside the source directory")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".titan-assets-", dir=destination.parent) as temporary:
        staged = Path(temporary) / "assets"
        shutil.copytree(source, staged)
        if destination.is_symlink() or destination.is_file():
            raise ValueError(f"asset output is not a directory: {destination}")
        if destination.exists():
            shutil.rmtree(destination)
        staged.rename(destination)


def browser(root, metadata, *, package_name, out_name, assets_source=None):
    """Build bindings and replace web/assets from assets_source (default root/assets)."""
    root = Path(root).resolve()
    resources = asset_source(root, assets_source)
    target = Path(metadata["target_directory"])
    package = package_by_name(metadata, package_name)
    libraries = [t for t in package["targets"] if "cdylib" in t["crate_types"]]
    if len(libraries) != 1:
        raise ValueError(f"expected one cdylib target in {package_name!r}")
    version = package_by_name(metadata, "wasm-bindgen")["version"]
    tool_root = target / "titan/tools"
    candidates = [tool_root / "bin/wasm-bindgen"]
    # Reuse an engine checkout's tool or PATH only when its version matches.
    engine = package_by_name(metadata, "titan")
    # Ship shared browser controls from the Cargo-resolved engine, including
    # when the game lives outside this checkout. The root page uses the source.
    shared_input = Path(engine["manifest_path"]).parent / "web/shared/input.mjs"
    output_input = root / "web/shared/input.mjs"
    if shared_input.resolve() != output_input.resolve():
        output_input.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(shared_input, output_input)
    # Standalone browser acceptance resolves this helper before Cargo metadata.
    process_source = Path(engine["manifest_path"]).parent / "scripts/acceptance_process.mjs"
    process_output = root / "scripts/acceptance_process.mjs"
    if process_source.resolve() != process_output.resolve():
        process_output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(process_source, process_output)
    candidates.append(Path(engine["manifest_path"]).parent / "target/titan/tools/bin/wasm-bindgen")
    if installed := shutil.which("wasm-bindgen"):
        candidates.append(Path(installed))
    bindgen = None
    for candidate in candidates:
        if candidate.is_file():
            result = processes.run([str(candidate), "--version"], capture_output=True, text=True, phase="build")
            if result.returncode == 0 and result.stdout.strip() == f"wasm-bindgen {version}":
                bindgen = candidate
                break
    if bindgen is None:
        processes.run(["cargo", "install", "wasm-bindgen-cli", "--version", version,
                        "--locked", "--root", str(tool_root), "--force"], cwd=root, check=True, phase="build")
        bindgen = tool_root / "bin/wasm-bindgen"
    processes.run(["rustup", "target", "add", "wasm32-unknown-unknown"], cwd=root, check=True, phase="build")
    processes.run(["cargo", "build", "--package", package_name, "--lib", "--target",
                    "wasm32-unknown-unknown", "--release"], cwd=root, check=True, phase="build")
    wasm = target / "wasm32-unknown-unknown/release" / (libraries[0]["name"].replace("-", "_") + ".wasm")
    for flavor, output in [("web", root / "web/inspector/pkg"), ("nodejs", target / "titan/browser-node")]:
        processes.run([str(bindgen), str(wasm), "--target", flavor, "--out-dir", str(output),
                        "--out-name", out_name], cwd=root, check=True, phase="build")

    copy_assets(resources, root / "web/assets")


def macos_app(root, metadata, argv=None, *, assets_source=None):
    """Bundle a binary/example and replace Resources/assets when a source exists."""
    root = Path(root).resolve()
    parser = argparse.ArgumentParser(description="Bundle a native Cargo binary as an unsigned local-development macOS app.")
    targets = parser.add_mutually_exclusive_group()
    targets.add_argument("--bin", help="Cargo binary target (default: play)")
    targets.add_argument("--example", help="Cargo example executable target")
    parser.add_argument("--name", default="Titan Game", help="Application display and directory name")
    parser.add_argument("--bundle-id", default="dev.titan.game", help="Distinct reverse-DNS bundle identifier")
    parser.add_argument("--release", action="store_true", help="Build an optimized binary")
    args = parser.parse_args(argv)
    if sys.platform != "darwin":
        parser.error("macOS app bundling requires a macOS host")
    if not args.name.strip() or any(character in args.name for character in "/:\\") or args.name in (".", ".."):
        parser.error("--name must be a nonempty application name without path separators")
    if not re.fullmatch(r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+", args.bundle_id):
        parser.error("--bundle-id must be a reverse-DNS identifier using letters, digits, hyphens and dots")
    package = next(package for package in metadata["packages"]
                   if Path(package["manifest_path"]).resolve() == root / "Cargo.toml")
    target_name = args.example or args.bin or "play"
    target_kind = "example" if args.example else "bin"
    if not any(target["name"] == target_name and target_kind in target["kind"]
               and "bin" in target["crate_types"] for target in package["targets"]):
        parser.error(f"no executable {target_kind} target named {target_name!r} in {package['name']}")
    resources = asset_source(root, assets_source)
    command = ["cargo", "build", "--package", package["name"], f"--{target_kind}", target_name, "--message-format=json-render-diagnostics"]
    if args.release:
        command.append("--release")
    executable = None
    build = processes.run(command, cwd=root, text=True, stdout=subprocess.PIPE, phase="build")
    for line in build.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            print(message["message"].get("rendered", ""), end="", file=sys.stderr)
        if (message.get("reason") == "compiler-artifact"
                and message.get("package_id") == package["id"]
                and message["target"]["name"] == target_name
                and message.get("executable")):
            executable = Path(message["executable"])
    if build.returncode:
        raise SystemExit("Cargo build failed; no app bundle was written")
    if executable is None:
        raise SystemExit("Cargo did not report the requested binary executable")
    # Cargo metadata honors CARGO_TARGET_DIR, and build JSON also handles a
    # configured target triple without guessing the executable's location.
    profile = "release" if args.release else "debug"
    bundle = Path(metadata["target_directory"]) / "macos-app" / profile / f"{args.name}.app"
    contents = bundle / "Contents"
    macos = contents / "MacOS"
    macos.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, macos / target_name)
    copy_assets(resources, contents / "Resources/assets")
    with (contents / "Info.plist").open("wb") as output:
        plistlib.dump({
            "CFBundleExecutable": target_name,
            "CFBundleIdentifier": args.bundle_id,
            "CFBundleName": args.name,
            "CFBundleDisplayName": args.name,
            "CFBundlePackageType": "APPL",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleShortVersionString": package["version"].split("-")[0].split("+")[0],
            "CFBundleVersion": package["version"].split("-")[0].split("+")[0],
            "NSHighResolutionCapable": True,
        }, output)
    print(bundle.resolve())
