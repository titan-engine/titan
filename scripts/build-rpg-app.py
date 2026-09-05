#!/usr/bin/env python3
"""Bundle the RPG player and its PNG resources as an unsigned local macOS app."""
from pathlib import Path
import sys
from titan_build import cargo_metadata, macos_app

if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    macos_app(root, cargo_metadata(root), [
        "--example", "play_rpg", "--name", "Titan RPG", "--bundle-id", "dev.titan.rpg",
        *sys.argv[1:],
    ], assets_source=root / "assets")
