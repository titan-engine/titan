#!/usr/bin/env python3
"""Bundle the adventure GPU player as an unsigned local macOS app."""
from titan_tools import ROOT, load

if __name__ == "__main__":
    tools, metadata = load()
    tools.macos_app(ROOT, metadata, features=("player",))
