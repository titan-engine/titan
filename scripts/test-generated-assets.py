#!/usr/bin/env python3
"""Verify the generated image cache survives process boundaries and damaged files."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def command(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, text=True,
                          capture_output=True, timeout=300, **kwargs).stdout


def main():
    metadata = json.loads(command(["cargo", "metadata", "--format-version", "1", "--no-deps"]))
    # Distinct tokens force actual build.rs executions without changing sources.
    # The cache is stable in OUT_DIR across these two invocations.
    for token in ("cold-check", "warm-check"):
        environment = dict(os.environ, TITAN_ASSET_BUILD_CHECK=token)
        command(["cargo", "build", "-p", "titan-generated-asset"], env=environment)
    binary = Path(metadata["target_directory"]) / "debug" / ("titan-generated-asset.exe" if os.name == "nt" else "titan-generated-asset")
    with tempfile.TemporaryDirectory(prefix="titan generated asset ") as directory:
        cache = Path(directory) / "cache"

        def run(expected, *args):
            report = json.loads(command([str(binary), "--cache-dir", str(cache), *args]))
            assert report["cache_outcome"] == expected, report
            assert report["parity"], report
            assert report["build_cache_outcome"] == "reused", report
            assert report["build_generation_count"] == 0, report
            assert report["before_access_generation_count"] == 0, report
            assert report["generation_count"] == (0 if expected == "reused" else 1), report
            assert report["startup_cache_outcome"] == "reused", report
            assert report["startup_generation_count"] == 0, report
            return report

        cold = run("generated")
        warm = run("reused")
        assert cold["cache_key"] == warm["cache_key"]
        assert cold["pixel_checksum"] == warm["pixel_checksum"]
        changed_input = run("generated", "--seed", "99")
        changed_version = run("generated", "--generator-version", "2")
        assert len({cold["cache_key"], changed_input["cache_key"], changed_version["cache_key"]}) == 3
        assert changed_input["pixel_checksum"] != cold["pixel_checksum"]
        assert changed_version["pixel_checksum"] == cold["pixel_checksum"]
        run("reused", "--seed", "99")
        run("reused", "--generator-version", "2")
        entry = Path(cold["cache_path"])
        for damage in (b"", b"corrupt PNG cache", b"x" * (1024 * 1024)):
            entry.write_bytes(damage)
            run("recovered")
            run("reused")
        print(json.dumps({"ok": True, "cold": cold, "warm": warm,
                          "verified": ["fresh-process reuse", "input invalidation",
                                       "version invalidation", "corrupt-entry recovery"]}, indent=2))


if __name__ == "__main__":
    main()
