#!/usr/bin/env python3
"""Build this game's WebAssembly and matching browser/Node bindings."""
from titan_tools import ROOT, load

if __name__ == "__main__":
    tools, metadata = load()
    package = next(p for p in metadata["packages"] if p["id"] == metadata["resolve"]["root"])
    tools.browser(ROOT, metadata, package_name=package["name"], out_name="titan_game")
    print("Built headless browser and Node bindings; run node scripts/test-browser.mjs.")
