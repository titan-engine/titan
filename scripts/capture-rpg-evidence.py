#!/usr/bin/env python3
"""Capture startup and reference-replay evidence from the built native RPG.

Build first with cargo build --locked -p titan-cli -p titan --bin titan --example procedural_rpg.
Uses only Python's standard library; all game interaction goes through the CLI.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import acceptance_process as processes
import tempfile
import time
import zlib

REPO = Path(__file__).resolve().parent.parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
if not TARGET.is_absolute():
    TARGET = REPO / TARGET


def png_from_ppm(source, destination, scale=1):
    magic, dimensions, maximum, pixels = source.read_bytes().split(b"\n", 3)
    width, height = map(int, dimensions.split())
    if magic != b"P6" or maximum != b"255" or len(pixels) != width * height * 3:
        raise ValueError("expected Titan's RGB8 PPM capture")

    def chunk(kind, payload):
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))

    if scale < 1:
        raise ValueError("scale must be positive")
    rows = bytearray()
    for y in range(height):
        row = pixels[y * width * 3:(y + 1) * width * 3]
        enlarged = b"".join(row[x:x + 3] * scale for x in range(0, len(row), 3))
        rows.extend((b"\0" + enlarged) * scale)
    width *= scale
    height *= scale
    destination.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new evidence directory")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix="titan-art-") as directory, tempfile.TemporaryFile() as log:
        project = Path(directory).resolve()
        runtime = processes.Popen([
            str(TARGET / "debug/examples/procedural_rpg"), "--serve", "--project", str(project),
            "--instance", "art-evidence", "--run-for-ms", "30000",
        ], project=project, instance="art-evidence", stdout=log, stderr=log)
        try:
            def call(*arguments):
                output = processes.run([
                    str(TARGET / "debug/titan"), "--project", str(project), "--instance", "art-evidence",
                    "--format", "json", *arguments,
                ], capture_output=True, text=True, check=True)
                result = json.loads(output.stdout)
                assert result["status"] == "success", result
                return result

            deadline = time.monotonic() + 10
            while not list((project / "target/titan/instances").glob("*.json")):
                if runtime.poll() is not None or time.monotonic() >= deadline:
                    log.seek(0)
                    raise RuntimeError("runtime failed to register: " + log.read().decode())
                time.sleep(0.02)

            evidence = {"source_revision": processes.check_output(
                ["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()}

            def capture(label):
                result = call("capture")
                capture = result["response"]
                png_from_ppm(Path(capture["artifact"]), args.output / (label + ".png"))
                if label == "startup":
                    # GitHub strips image-rendering CSS. Bake in nearest-neighbor
                    # scaling; 8x stays crisp at 640 CSS pixels on a 2x display.
                    png_from_ppm(Path(capture["artifact"]), args.output / "startup-preview.png", scale=8)
                return {"frame": result["observed_frame"], "checksum": capture["checksum"],
                        "width": capture["width"], "height": capture["height"], "image": label + ".png"}

            evidence["startup"] = capture("startup")
            frame = 0
            for action, count in [("right", 2), ("down", 3), ("right", 6)]:
                for _ in range(count):
                    frame += 1
                    call("input", str(frame), "--actions", json.dumps({action: {"kind": "button", "value": True}}))
            call("step", "11")
            evidence["completed"] = capture("completed")
            entities = call("entities")["response"]["entities"]
            shrine = next(entity for entity in entities if entity["name"] == "shrine")
            assert len(entities) == 2
            assert any(name.endswith("::ActiveShrine") for name in shrine["components"])
            assert evidence["completed"]["frame"] == 11
            evidence["assertions"] = {"remaining_entities": 2, "shrine_active": True, "replay_ticks": 11}
            (args.output / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
            print(json.dumps(evidence, indent=2))
        finally:
            processes.terminate(runtime)


if __name__ == "__main__":
    main()
