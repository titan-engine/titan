#!/usr/bin/env python3
"""Reproduce issue #39 native evidence; build CLI/RPG first. Uses owned games."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

REPO = next(parent for parent in Path(__file__).resolve().parents
            if (parent / "Cargo.toml").is_file() and (parent / "scripts").is_dir())
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
if not TARGET.is_absolute():
    TARGET = REPO / TARGET
CLI = TARGET / "debug/titan"
GAME = TARGET / "debug/examples/procedural_rpg"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        default=REPO / "target/evidence/inspection-repair/native-output.json")
    options = parser.parse_args()
    if not __debug__:
        parser.error("Python assertions must be enabled (do not use -O)")
    with tempfile.TemporaryDirectory(prefix="titan-repair-") as directory:
        project = Path(directory).resolve()
        owned = []
        evidence = {}

        def cli(*args, instance=None, success=True):
            command = [str(CLI), "--format", "json", "--project", str(project)]
            if instance:
                command += ["--instance", instance]
            result = subprocess.run(command + list(args), capture_output=True,
                                    text=True, timeout=10, check=False)
            value = json.loads(result.stdout)
            assert (result.returncode == 0) == success, value
            assert value["status"] == ("success" if success else "failure"), value
            return value

        def start(instance):
            process = subprocess.Popen(
                [str(GAME), "--serve", "--project", str(project), "--instance",
                 instance, "--run-for-ms", "30000"], cwd=REPO,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            owned.append(process)
            deadline = time.monotonic() + 10
            while instance not in [item["instance_id"] for item in cli("instances")["instances"]]:
                assert process.poll() is None, "runtime exited before discovery"
                assert time.monotonic() < deadline, "runtime discovery timed out"
                time.sleep(0.02)

        def bundle(response):
            manifest = Path(response["error"]["details"]["diagnostic_bundle"])
            data = json.loads(manifest.read_text())
            api = manifest.parent / "api.txt"
            return {"response": data["response"], "local_error": data["local_error"],
                    "api_text": api.read_text() if api.exists() else None}

        try:
            start("repair-a")
            player = cli("entities", "--name", "player", instance="repair-a")["response"]["entities"][0]["id"]
            args = [str(player["index"]), str(player["generation"])]
            before = cli("entity", *args, instance="repair-a")
            component = next(key for key in before["response"]["components"] if key.endswith("::Position"))
            denied = cli("set-field", *args, component, "x", "--value", "3",
                         instance="repair-a", success=False)
            assert denied["error"]["code"] == "mutation_disabled"
            after = cli("entity", *args, instance="repair-a")
            assert before["response"] == after["response"]
            assert denied["observed_frame"] == after["observed_frame"] == 0
            assert denied["state_revision"] == after["state_revision"] == 0
            evidence["mutation"] = {
                "before": before, "failure": denied, "bundle": bundle(denied),
                "capabilities": cli("capabilities", instance="repair-a"),
                "after": after}
            cli("invoke", "resume", instance="repair-a")
            uncontrolled = cli("step", "1", instance="repair-a", success=False)
            assert uncontrolled["error"]["code"] == "not_controlled"
            evidence["clock"] = {
                "failure": uncontrolled, "bundle": bundle(uncontrolled),
                "status": cli("status", instance="repair-a"),
                "capabilities": cli("capabilities", instance="repair-a"),
                "commands": cli("commands", instance="repair-a")}
            evidence["clock"]["pause"] = cli("invoke", "pause", instance="repair-a")
            evidence["clock"]["paused_status"] = cli("status", instance="repair-a")
            evidence["clock"]["step_after_pause"] = cli("step", "1", instance="repair-a")
            start("repair-b")
            ambiguous = cli("status", success=False)
            assert ambiguous["error"]["code"] == "ambiguous_target"
            instances = cli("instances")
            assert len(instances["instances"]) == 2
            assert all("token" not in item for item in instances["instances"])
            selected = cli("status", instance="repair-a")
            evidence["ambiguity"] = {"failure": ambiguous, "bundle": bundle(ambiguous),
                                     "instances": instances, "selected_status": selected}
            # Only known volatile fields are normalized; never read raw registrations.
            def sanitize(value):
                if isinstance(value, dict):
                    return {key: ("<volatile>" if key in {"request_id", "pid", "endpoint"}
                                  else "<bundle.json>" if key == "diagnostic_bundle"
                                  else sanitize(item)) for key, item in value.items()}
                if isinstance(value, list):
                    return [sanitize(item) for item in value]
                if isinstance(value, str):
                    return value.replace(str(project), "<project>")
                return value
            options.output.parent.mkdir(parents=True, exist_ok=True)
            options.output.write_text(json.dumps(sanitize(evidence), indent=2, sort_keys=True) + "\n")
            print(f"Native inspection evidence: {options.output}")
        finally:
            for process in owned:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
            for process in owned:
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            assert not list((project / "target/titan/instances").glob("*.json")), "registration cleanup failed"


if __name__ == "__main__":
    main()
