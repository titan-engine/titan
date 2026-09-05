#!/usr/bin/env python3
"""Copy the starter outside the checkout and drive its public native protocol."""
import argparse
import json
import os
from pathlib import Path
import acceptance_process as processes
import tempfile
import time

REPO = Path(__file__).resolve().parent.parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")).resolve()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--browser", action="store_true", help="also build and test copied WASM")
    options = parser.parse_args()
    processes.run(["cargo", "build", "-p", "titan-cli"], cwd=REPO, check=True, phase="build")
    with tempfile.TemporaryDirectory(prefix="titan-starter-") as directory:
        project = Path(directory) / "my-game"
        processes.run(["python3", str(REPO / "scripts/create-game.py"), str(project)], check=True)
        # Running setup twice must preserve a contributor's existing game files.
        sentinel = project / "keep-my-work.txt"
        sentinel.write_text("work in progress")
        duplicate = processes.run(
            ["python3", str(REPO / "scripts/create-game.py"), str(project)],
            text=True, capture_output=True)
        assert duplicate.returncode != 0
        assert "destination already exists" in duplicate.stderr
        assert sentinel.read_text() == "work in progress"
        assert "examples/support" not in "\n".join(p.read_text() for p in (project / "src").rglob("*.rs"))
        env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET / "starter-smoke"))
        for command in (["cargo", "fmt", "--all", "--check"],
                        ["cargo", "test", "--all-targets"],
                        ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
                        ["cargo", "build", "--bins"]):
            processes.run(command, cwd=project, env=env, check=True, phase="build")
        if options.browser:
            for command in (["cargo", "check", "--lib", "--target", "wasm32-unknown-unknown"],
                            ["python3", "scripts/build-browser.py"],
                            ["node", "scripts/test-browser.mjs"],
                            ["node", "--test", "web/inspector/bridge.test.mjs"]):
                processes.run(command, cwd=project, env=env, check=True, phase="runtime" if command[0] == "node" else "build")
        executable = TARGET / "starter-smoke/debug/titan-game"
        with tempfile.TemporaryFile(mode="w+") as log:
            process = processes.Popen([str(executable), "--serve", "--allow-mutation",
                "--instance", "starter-smoke", "--run-for-ms", "30000"], cwd=project,
                project=project, instance="starter-smoke", stdout=log, stderr=log)
            try:
                def cli(*args, success=True):
                    result = processes.run([str(TARGET / "debug/titan"), "--format", "json",
                        "--project", str(project), "--instance", "starter-smoke", *args],
                        text=True, capture_output=True)
                    value = json.loads(result.stdout)
                    assert (result.returncode == 0) == success, value
                    assert value["status"] == ("success" if success else "failure"), value
                    return value

                deadline = time.monotonic() + 10
                while not cli("instances")["instances"]:
                    if process.poll() is not None or time.monotonic() > deadline:
                        log.seek(0)
                        raise AssertionError(log.read())
                    time.sleep(.02)
                assert cli("status")["observed_frame"] == 0
                assert "step" in cli("capabilities")["response"]["operations"]
                player = next(e["id"] for e in cli("entities")["response"]["entities"] if e["name"] == "player")
                def details():
                    return cli("entity", str(player["index"]), str(player["generation"]))["response"]
                before = details()
                position = next(k for k in before["components"] if k.endswith("::Position"))
                capture = cli("capture")["response"]
                assert Path(capture["artifact"]).read_bytes().startswith(b"P6\n")
                cli("input", "1", "--actions", '{"right":{"kind":"button","value":true}}')
                assert cli("step", "1")["observed_frame"] == 1
                assert details()["components"][position]["x"] > before["components"][position]["x"]
                assert cli("capture")["response"]["checksum"] != capture["checksum"]
                assert any(c["name"] == "restart" for c in cli("commands")["response"]["commands"])
                cli("invoke", "restart")
                assert cli("capture")["response"]["checksum"] == capture["checksum"]
                field = before["component_fields"][position]["x"]
                args = ("set-field", str(player["index"]), str(player["generation"]), position, "x", "--value")
                cli(*args, str(int(field["minimum"])))
                rejected = cli(*args, str(int(field["maximum"]) + 1), success=False)
                assert rejected["error"]["code"] == "invalid_value"
                bundle_path = Path(rejected["error"]["details"]["diagnostic_bundle"])
                bundle = json.loads(bundle_path.read_text())
                assert bundle["history"]["accepted_inputs"]
                assert bundle["world_state"]
                assert (bundle_path.parent / "api.txt").exists()
                assert (bundle_path.parent / bundle["capture"]["artifact"]).exists()
                assert processes.graceful_shutdown(process) == 0
                assert cli("instances")["instances"] == []
            finally:
                processes.terminate(process)
    print("Copied starter passed: build, discovery, input, step, capture, restart, fields, diagnostics, shutdown.")


if __name__ == "__main__":
    main()
