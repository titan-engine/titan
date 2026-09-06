#!/usr/bin/env python3
"""Build this game's WebAssembly and matching browser/Node bindings."""
from titan_tools import ROOT, load
import shutil

if __name__ == "__main__":
    tools, metadata = load()
    package = next(p for p in metadata["packages"] if p["id"] == metadata["resolve"]["root"])
    tools.browser(ROOT, metadata, package_name=package["name"], out_name="titan_game", features=("player",))
    shutil.copyfile(ROOT / "tests/puzzle-solution.json", ROOT / "web/play/puzzle-solution.json")
    for name in ("block-solution.json", "block-intermediate-solution.json"):
        shutil.copyfile(ROOT / "tests" / name, ROOT / "web/play" / name)
    print("Built playable browser and Node bindings; serve web/ and open /play/.")
