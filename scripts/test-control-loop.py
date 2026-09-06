#!/usr/bin/env python3
"""Exercise the native RPG through separate CLI processes (standard library only)."""
import json
import os
from pathlib import Path
import acceptance_process as processes
import tempfile
import time
import urllib.error
import urllib.request

from acceptance_evidence import FailureEvidence

REPO = Path(__file__).resolve().parent.parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
if not TARGET.is_absolute():
    TARGET = REPO / TARGET
CLI = TARGET / "debug" / "titan"
GAME = TARGET / "debug" / "examples" / "procedural_rpg"


def main(failures):
    with failures.runtime_log() as build_log:
        failures.record_command(["cargo", "build", "RPG and CLI"], None)
        processes.run(
            ["cargo", "build", "-p", "titan-cli", "-p", "titan", "--example", "procedural_rpg", "--bin", "titan"],
            cwd=REPO,
            check=True, phase="build",
            stdout=build_log, stderr=build_log,
        )
    with tempfile.TemporaryDirectory(prefix="titan-control-loop-") as directory:
        project = Path(directory).resolve()
        with failures.runtime_log() as log:
            game = processes.Popen(
                [str(GAME), "--serve", "--project", str(project), "--instance", "acceptance", "--allow-mutation", "--run-for-ms", "30000"],
                cwd=REPO,
                project=project, instance="acceptance", stdout=log,
                stderr=log,
            )
            failures.record_process(game)
            try:
                def cli(*arguments, success=True):
                    failures.record_command(arguments, None)
                    output = processes.run(
                        [str(CLI), "--format", "json", "--project", str(project), "--instance", "acceptance", *arguments],
                        capture_output=True,
                        text=True,
                    )
                    failures.record_command(arguments, output)
                    result = json.loads(output.stdout)  # Rejects extra stdout content.
                    failures.observe(result)
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
                assert len([entity for entity in entities if not (entity.get("name") or "").startswith("ui/journal/")]) == 6
                shrine = next(entity["id"] for entity in entities if entity["name"] == "shrine")
                assert any(command['name'] == 'spawn_shard' for command in cli('commands')['response']['commands'])
                frame = 0
                for action, ticks in [("right", 2), ("down", 3), ("right", 6)]:
                    for _ in range(ticks):
                        frame += 1
                        cli("input", str(frame), "--actions", json.dumps({action: {"kind": "button", "value": True}}))
                stepped = cli("step", "11")
                assert stepped["observed_frame"] == 11
                details = cli("entity", str(shrine["index"]), str(shrine["generation"]))["response"]
                assert any(name.endswith("::ActiveShrine") for name in details["components"])
                assert {entity["name"] for entity in cli("entities")["response"]["entities"] if not (entity.get("name") or "").startswith("ui/journal/")} == {"player", "shrine", "ui/quest"}
                capture = cli("capture")["response"]
                assert capture["checksum"] == "f7a298f62ad75c1c", capture
                artifact = Path(capture["artifact"])
                assert artifact.is_absolute(), capture
                assert artifact.read_bytes().startswith(b"P6\n160 112\n255\n")
                rejected = cli("invoke", "spawn_shard", "--arguments", '{"x":-1,"y":0}', success=False)
                assert rejected["error"]["code"] == "invalid_value"
                assert rejected["state_revision"] == stepped["state_revision"]
                manifest = Path(rejected["error"]["details"]["diagnostic_bundle"])
                assert manifest.is_absolute() and manifest.is_file()
                bundle = json.loads(manifest.read_text())
                assert bundle["response"]["observed_frame"] == 11
                assert bundle["response"]["state_revision"] == stepped["state_revision"]
                assert bundle["request"]["request"]["type"] == "invoke"
                assert bundle["response"]["error"]["code"] == "invalid_value"
                assert len(bundle["history"]["accepted_inputs"]) == 11
                assert bundle["world_state"]["quest"] == {"collected_shards": 3, "shrine_active": True}
                assert bundle["capture"]["checksum"] == "f7a298f62ad75c1c"
                assert (manifest.parent / bundle["capture"]["artifact"]).read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
                assert "spawn_shard" in (manifest.parent / "api.txt").read_text()
                assert "request" in bundle["timings_us"]
                failures.checkpoint("diagnostic")
                # The CLI preserves the richer runtime bundle instead of writing a duplicate.
                assert len(list((project / "target/titan/diagnostics").glob("*/bundle.json"))) == 1
                cli("invoke", "spawn_shard", "--arguments", '{"x":0,"y":0}')
                assert cli("capture")["response"]["checksum"] != capture["checksum"]
                assert cli("status")["observed_frame"] == 11
                assert cli("input", "12", "--actions", "[]", success=False)["error"]["code"] == "invalid_value"
                player = next(entity["id"] for entity in entities if entity["name"] == "player")
                player_details = cli("entity", str(player["index"]), str(player["generation"]))["response"]
                position_type = next(name for name in player_details["components"] if name.endswith("::Position"))
                assert position_type == "procedural_rpg::game::Position"
                assert player_details["components"][position_type] == {"x": 10, "y": 5}
                assert player_details["component_fields"][position_type]["x"]["maximum"] == 19
                changed = cli("set-field", str(player["index"]), str(player["generation"]), position_type, "x", "--value", "9")
                assert changed["observed_frame"] == 11
                assert cli("entity", str(player["index"]), str(player["generation"]))["response"]["components"][position_type]["x"] == 9
                for value in ["20", '"9"']:
                    invalid = cli("set-field", str(player["index"]), str(player["generation"]), position_type, "x", "--value", value, success=False)
                    assert invalid["error"]["code"] == "invalid_value"
                    assert invalid["state_revision"] == changed["state_revision"]
                assert cli("entity", str(player["index"]), str(player["generation"]))["response"]["components"][position_type]["x"] == 9
                registration_path = next((project / "target/titan/instances").glob("*.json"))
                registration = json.loads(registration_path.read_text())
                failures.redact_secret(registration["token"])
                request = urllib.request.Request(
                    registration["endpoint"],
                    data=json.dumps({"schema_version": registration["schema_version"] + 1, "request_id": "mismatch", "request": {"type": "status"}}).encode(),
                    headers={"Authorization": "Bearer " + registration["token"], "Content-Type": "application/json"},
                )
                with urllib.request.urlopen(request, timeout=5) as response:
                    mismatch = json.load(response)
                assert mismatch["error"]["code"] == "protocol_mismatch"
                assert mismatch["request_id"] == "mismatch"
                assert processes.graceful_shutdown(game) == 0
                assert not list((project / "target/titan/instances").glob("*.json"))
                assert cli("instances")["instances"] == []
            finally:
                processes.terminate(game)
    print("Native CLI control loop passed: replay, inspection, commands, fields, exact capture, diagnostics, shutdown.")


if __name__ == "__main__":
    with FailureEvidence("rpg-control", repo=REPO) as failures:
        main(failures)
