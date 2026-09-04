#!/usr/bin/env python3
"""Copy the starter outside the checkout and drive its public native protocol."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

REPO = Path(__file__).resolve().parent.parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")).resolve()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--browser", action="store_true", help="also build and test copied WASM")
    options = parser.parse_args()
    subprocess.run(["cargo", "build", "-p", "titan-cli"], cwd=REPO, check=True)
    with tempfile.TemporaryDirectory(prefix="titan-starter-") as directory:
        project = Path(directory) / "my-game"
        shutil.copytree(REPO / "starters/minimal", project,
                        ignore=shutil.ignore_patterns("target", "pkg", "__pycache__"))
        manifest = project / "Cargo.toml"
        # The same path configuration documented in the starter README.
        text = manifest.read_text()
        text = re.sub(r'path = "(\.\./\.\.[^"]*)"',
                      lambda m: 'path = ' + json.dumps(str((REPO / "starters/minimal" / m[1]).resolve())), text)
        manifest.write_text(text)
        assert "examples/support" not in "\n".join(p.read_text() for p in (project / "src").rglob("*.rs"))
        env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET / "starter-smoke"))
        for command in (["cargo", "fmt", "--all", "--check"],
                        ["cargo", "test", "--all-targets"],
                        ["cargo", "clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
                        ["cargo", "build", "--bins"]):
            subprocess.run(command, cwd=project, env=env, check=True, timeout=600)
        if options.browser:
            for command in (["cargo", "check", "--lib", "--target", "wasm32-unknown-unknown"],
                            ["python3", "scripts/build-browser.py"],
                            ["node", "scripts/test-browser.mjs"],
                            ["node", "--test", "web/inspector/bridge.test.mjs"]):
                subprocess.run(command, cwd=project, env=env, check=True, timeout=600)
        executable = TARGET / "starter-smoke/debug/titan-game"
        with tempfile.TemporaryFile(mode="w+") as log:
            process = subprocess.Popen([str(executable), "--serve", "--allow-mutation",
                "--instance", "starter-smoke", "--run-for-ms", "30000"], cwd=project,
                stdout=log, stderr=log)
            try:
                def cli(*args, success=True):
                    result = subprocess.run([str(TARGET / "debug/titan"), "--format", "json",
                        "--project", str(project), "--instance", "starter-smoke", *args],
                        text=True, capture_output=True, timeout=10)
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
                process.terminate()
                assert process.wait(timeout=10) == 0
                assert cli("instances")["instances"] == []
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
    print("Copied starter passed: build, discovery, input, step, capture, restart, fields, diagnostics, shutdown.")


if __name__ == "__main__":
    main()
