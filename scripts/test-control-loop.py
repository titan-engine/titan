#!/usr/bin/env python3
"""Exercise the native RPG through separate CLI processes (standard library only)."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

REPO = Path(__file__).resolve().parent.parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
if not TARGET.is_absolute():
    TARGET = REPO / TARGET
CLI = TARGET / "debug" / "titan"
GAME = TARGET / "debug" / "examples" / "procedural_rpg"


def main():
    subprocess.run(
        ["cargo", "build", "-p", "titan-cli", "-p", "titan", "--example", "procedural_rpg", "--bin", "titan"],
        cwd=REPO,
        check=True,
    )
    with tempfile.TemporaryDirectory(prefix="titan-control-loop-") as directory:
        project = Path(directory).resolve()
        with tempfile.TemporaryFile(mode="w+") as log:
            game = subprocess.Popen(
                [str(GAME), "--serve", "--project", str(project), "--instance", "acceptance", "--run-for-ms", "30000"],
                cwd=REPO,
                stdout=log,
                stderr=log,
            )
            try:
                def cli(*arguments, success=True):
                    output = subprocess.run(
                        [str(CLI), "--format", "json", "--project", str(project), "--instance", "acceptance", *arguments],
                        capture_output=True,
                        text=True,
                        timeout=10,
                    )
                    result = json.loads(output.stdout)  # Rejects extra stdout content.
                    assert (output.returncode == 0) == success, result
                    assert result["status"] == ("success" if success else "failure"), result
                    return result

                deadline = time.monotonic() + 10
                while True:
                    instances = cli("instances")["instances"]
                    if instances:
                        break
                    if game.poll() is not None or time.monotonic() >= deadline:
                        log.seek(0)
                        raise AssertionError("headless game did not register: " + log.read())
                    time.sleep(0.02)
                assert len(instances) == 1
                assert "token" not in instances[0]
                capabilities = cli("capabilities")["response"]["operations"]
                assert {"inspect", "invoke", "inject_input", "step", "capture"} <= set(capabilities)
                entities = cli("entities")["response"]["entities"]
                assert len(entities) == 5
                shrine = next(entity["id"] for entity in entities if entity["name"] == "shrine")
                assert cli("commands")["response"]["commands"][0]["name"] == "spawn_shard"
                frame = 0
                for action, ticks in [("right", 2), ("down", 3), ("right", 6)]:
                    for _ in range(ticks):
                        frame += 1
                        cli("input", str(frame), "--actions", json.dumps({action: {"kind": "button", "value": True}}))
                stepped = cli("step", "11")
                assert stepped["observed_frame"] == 11
                details = cli("entity", str(shrine["index"]), str(shrine["generation"]))["response"]
                assert any(name.endswith("::ActiveShrine") for name in details["components"])
                assert len(cli("entities")["response"]["entities"]) == 2
                capture = cli("capture")["response"]
                assert capture["checksum"] == "98618cd721c5b52d", capture
                artifact = Path(capture["artifact"])
                assert artifact.is_absolute(), capture
                assert artifact.read_bytes().startswith(b"P6\n160 112\n255\n")
                rejected = cli("invoke", "spawn_shard", "--arguments", '{"x":-1,"y":0}', success=False)
                assert rejected["error"]["code"] == "invalid_value"
                assert rejected["state_revision"] == stepped["state_revision"]
                cli("invoke", "spawn_shard", "--arguments", '{"x":0,"y":0}')
                assert cli("capture")["response"]["checksum"] != capture["checksum"]
                assert cli("status")["observed_frame"] == 11
                assert cli("input", "12", "--actions", "[]", success=False)["error"]["code"] == "invalid_value"
                registration_path = next((project / "target/titan/instances").glob("*.json"))
                registration = json.loads(registration_path.read_text())
                request = urllib.request.Request(
                    registration["endpoint"],
                    data=json.dumps({"schema_version": registration["schema_version"] + 1, "request_id": "mismatch", "request": {"type": "status"}}).encode(),
                    headers={"Authorization": "Bearer " + registration["token"], "Content-Type": "application/json"},
                )
                with urllib.request.urlopen(request, timeout=5) as response:
                    mismatch = json.load(response)
                assert mismatch["error"]["code"] == "protocol_mismatch"
                assert mismatch["request_id"] == "mismatch"
                game.terminate()
                assert game.wait(timeout=10) == 0
                assert not list((project / "target/titan/instances").glob("*.json"))
                assert cli("instances")["instances"] == []
            finally:
                if game.poll() is None:
                    game.terminate()
                    try:
                        game.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        game.kill()
                        game.wait()
    print("Native CLI control loop passed: replay, inspection, command, exact capture, errors, shutdown.")


if __name__ == "__main__":
    main()
