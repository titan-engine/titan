#!/usr/bin/env python3
"""Create a standalone game with dependencies pointing at this Titan checkout."""
import argparse
import json
from pathlib import Path
import re
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new game directory (must not exist)")
    options = parser.parse_args()
    # Do not resolve the final path: a dangling symlink must also be refused.
    destination = options.destination.expanduser().absolute()
    source = Path(__file__).resolve().parent.parent / "starters/minimal"
    if destination.exists() or destination.is_symlink():
        parser.error(f"destination already exists: {destination}; choose a new directory")
    if destination.resolve().is_relative_to(source):
        parser.error("destination must be outside the starter template")

    # copytree also refuses an existing destination if it appears after the check.
    shutil.copytree(source, destination,
                    ignore=shutil.ignore_patterns("target", "pkg", "__pycache__", ".DS_Store"))
    manifest = destination / "Cargo.toml"
    manifest.write_text(re.sub(
        r'path = "(\.\./\.\.[^"]*)"',
        lambda match: 'path = ' + json.dumps(str((source / match[1]).resolve()), ensure_ascii=False),
        manifest.read_text(),
    ))
    print(f"Created game: {destination}")
    print("Open that directory and initialize dependencies: cargo generate-lockfile")
    print("Then run: cargo run --locked --bin play")
    print(f"Keep this Titan checkout at: {source.parent.parent}")


if __name__ == "__main__":
    main()
