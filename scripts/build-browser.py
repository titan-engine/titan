#!/usr/bin/env python3
"""Build matching wasm-bindgen web and Node packages for the RPG inspector."""
from pathlib import Path
from titan_build import browser, cargo_metadata

if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    browser(root, cargo_metadata(root), package_name="titan-browser", out_name="titan_browser", assets_source=root / "assets")
    print("Browser packages built. Serve web/inspector over localhost HTTP to open the inspector.")
