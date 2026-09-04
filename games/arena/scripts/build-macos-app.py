#!/usr/bin/env python3
"""Bundle a native Cargo binary as an unsigned local-development macOS app."""
import argparse
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin", default="play", help="Cargo binary target (default: play)")
    parser.add_argument("--name", default="Titan Game", help="Application display and directory name")
    parser.add_argument("--bundle-id", default="dev.titan.game", help="Distinct reverse-DNS bundle identifier")
    parser.add_argument("--release", action="store_true", help="Build an optimized binary")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("macOS app bundling requires a macOS host")
    if not args.name.strip() or any(character in args.name for character in "/:\\") or args.name in (".", ".."):
        parser.error("--name must be a nonempty application name without path separators")
    if not re.fullmatch(r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+", args.bundle_id):
        parser.error("--bundle-id must be a reverse-DNS identifier using letters, digits, hyphens and dots")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT, text=True,
    ))
    package = next(package for package in metadata["packages"]
                   if Path(package["manifest_path"]).resolve() == ROOT / "Cargo.toml")
    if not any(target["name"] == args.bin and "bin" in target["kind"] for target in package["targets"]):
        parser.error(f"no binary target named {args.bin!r} in {package['name']}")
    command = ["cargo", "build", "--package", package["name"], "--bin", args.bin, "--message-format=json-render-diagnostics"]
    if args.release:
        command.append("--release")
    executable = None
    with subprocess.Popen(command, cwd=ROOT, text=True, stdout=subprocess.PIPE) as build:
        for line in build.stdout:
            message = json.loads(line)
            if message.get("reason") == "compiler-message":
                print(message["message"].get("rendered", ""), end="", file=sys.stderr)
            if (message.get("reason") == "compiler-artifact"
                    and message.get("package_id") == package["id"]
                    and message["target"]["name"] == args.bin
                    and message.get("executable")):
                executable = Path(message["executable"])
        if build.wait():
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
    shutil.copy2(executable, macos / args.bin)
    with (contents / "Info.plist").open("wb") as output:
        plistlib.dump({
            "CFBundleExecutable": args.bin,
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


if __name__ == "__main__":
    main()
