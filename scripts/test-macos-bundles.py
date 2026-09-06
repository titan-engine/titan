#!/usr/bin/env python3
"""Build native .app bundles from external copies; requires macOS, no GPU."""
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import acceptance_process as processes
import sys
import tempfile

REPO = Path(__file__).resolve().parent.parent


def main():
    if sys.platform != "darwin":
        raise SystemExit("macOS bundle integration checks require macOS")
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")).resolve()
    env = dict(os.environ, CARGO_TARGET_DIR=str(target / "macos-bundle-smoke"))
    with tempfile.TemporaryDirectory(prefix="titan-app-copy-") as directory:
        for source, name, bundle_id in [
            (REPO / "starters/minimal", "Titan Starter Smoke", "dev.titan.starter-smoke"),
            (REPO / "games/arena", "Titan Arena Smoke", "dev.titan.arena-smoke"),
        ]:
            project = Path(directory) / source.name
            shutil.copytree(source, project,
                            ignore=shutil.ignore_patterns("target", "pkg", "__pycache__"))
            manifest = project / "Cargo.toml"
            manifest.write_text(re.sub(r'path = "(\.\./\.\.[^"]*)"',
                lambda m: 'path = ' + json.dumps(str((source / m[1]).resolve())),
                manifest.read_text()))
            # Initialize the copied project after deliberately rewriting its manifest.
            processes.run(["cargo", "generate-lockfile"], cwd=project, env=env,
                          check=True, phase="build")
            result = processes.run([sys.executable, "scripts/build-macos-app.py",
                "--name", name, "--bundle-id", bundle_id], cwd=project, env=env,
                check=True, text=True, stdout=subprocess.PIPE, phase="build")
            bundle = Path(result.stdout.strip().splitlines()[-1])
            assert bundle.is_absolute() and bundle.suffix == ".app", result.stdout
            with (bundle / "Contents/Info.plist").open("rb") as file:
                info = plistlib.load(file)
            assert info["CFBundleIdentifier"] == bundle_id
            assert info["CFBundlePackageType"] == "APPL"
            # Move the bundle away from build outputs and rename it: launch must
            # resolve its own embedded executable, without the source checkout.
            renamed = Path(directory) / (name + " Renamed.app")
            shutil.copytree(bundle, renamed)
            executable = renamed / "Contents/MacOS" / info["CFBundleExecutable"]
            assert executable.is_file() and os.access(executable, os.X_OK)
            help_result = processes.run([str(executable), "--help"], cwd=directory,
                check=True, text=True, capture_output=True)
            assert "--frames" in help_result.stdout
            print(f"Passed external-copy bundle: {name}")


if __name__ == "__main__":
    main()
