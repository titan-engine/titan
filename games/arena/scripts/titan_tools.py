"""Locate public Titan build helpers through this game's Cargo dependencies."""
import importlib.util
import json
import math
import os
import signal
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent


def metadata_bootstrap():
    """Bound dependency resolution before the engine's process helper is locatable."""
    timeout = float(os.environ.get("TITAN_BUILD_TIMEOUT_SECONDS", "1200"))
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("TITAN_BUILD_TIMEOUT_SECONDS must be finite and positive")
    inherited = os.environ.get("TITAN_ACCEPTANCE_DEADLINE_EPOCH")
    if inherited is not None:
        deadline = float(inherited)
        if not math.isfinite(deadline) or deadline <= 0:
            raise ValueError("TITAN_ACCEPTANCE_DEADLINE_EPOCH must be finite and positive")
        timeout = min(timeout, deadline - time.time())
    if timeout <= 0:
        raise RuntimeError("build phase deadline expired before cargo metadata")
    environment = dict(os.environ, TITAN_ACCEPTANCE_DEADLINE_EPOCH=str(time.time() + timeout - min(5, timeout / 2)))
    command = ["cargo", "metadata", "--format-version", "1", "--filter-platform", "wasm32-unknown-unknown"]
    process = subprocess.Popen(command, cwd=ROOT, text=True, stdout=subprocess.PIPE,
                               start_new_session=True, env=environment)
    previous_handler = signal.getsignal(signal.SIGTERM)
    def interrupted(signum, frame):
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, interrupted)
    try:
        output, _ = process.communicate(timeout=timeout)
        if process.returncode:
            raise subprocess.CalledProcessError(process.returncode, command, output)
        return json.loads(output)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"build phase timed out after {timeout:g}s: cargo metadata") from error
    finally:
        signal.signal(signal.SIGTERM, previous_handler)
        # The leader may already have exited while a descendant holds stdout.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.stdout.close()
        process.wait(timeout=5)


def load():
    metadata = metadata_bootstrap()
    engines = [p for p in metadata["packages"] if p["name"] == "titan"]
    if len(engines) != 1:
        raise SystemExit("Expected one resolved titan dependency for build tooling")
    path = Path(engines[0]["manifest_path"]).parent / "scripts/titan_build.py"
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("titan_build", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, metadata
