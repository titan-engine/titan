"""Locate public Titan build helpers through this game's Cargo dependencies."""
import importlib.util
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def load():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"],
        cwd=ROOT, text=True,
    ))
    engines = [p for p in metadata["packages"] if p["name"] == "titan"]
    if len(engines) != 1:
        raise SystemExit("Expected one resolved titan dependency for build tooling")
    path = Path(engines[0]["manifest_path"]).parent / "scripts/titan_build.py"
    spec = importlib.util.spec_from_file_location("titan_build", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, metadata
