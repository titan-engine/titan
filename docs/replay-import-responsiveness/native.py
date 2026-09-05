#!/usr/bin/env python3
"""Measure arena replay verification and native control responsiveness.

Build optimized arena binaries and titan-cli first. This opens the real GPU
window host; its control queue is the responsiveness surface under test.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import tempfile
import time


REPO = Path(__file__).resolve().parents[2]
GAME = REPO / "games/arena"
MAX_BYTES = 2 * 1024 * 1024


def metadata(manifest):
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1",
         "--manifest-path", str(manifest)], cwd=REPO, check=True,
        capture_output=True, text=True, timeout=60)
    return json.loads(result.stdout)


ROOT_TARGET = Path(metadata(REPO / "Cargo.toml")["target_directory"])
GAME_TARGET = Path(metadata(GAME / "Cargo.toml")["target_directory"])
CLI = ROOT_TARGET / "release/titan"
HEADLESS = GAME_TARGET / "release/titan-game"
PLAYER = GAME_TARGET / "release/play"
REPLAY = GAME_TARGET / "release/replay"


def run(command, **kwargs):
    return subprocess.run(command, cwd=GAME, timeout=10, check=False, **kwargs)


def command(instance, project, *args):
    return [str(CLI), "--format", "json", "--project", str(project),
            "--instance", instance, *map(str, args)]


def call(instance, project, *args, success=True):
    result = run(command(instance, project, *args), capture_output=True, text=True)
    value = json.loads(result.stdout)
    assert (result.returncode == 0) == success, value
    assert value["status"] == ("success" if success else "failure"), value
    return value


def start_host(binary, arguments, instance, project):
    process = subprocess.Popen(
        [str(binary), *arguments, "--project", str(project), "--instance", instance,
         "--run-for-ms", "120000"], cwd=GAME, stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, start_new_session=True)
    deadline = time.monotonic() + 10
    while True:
        result = run(command(instance, project, "instances"), capture_output=True, text=True)
        if result.returncode == 0:
            values = json.loads(result.stdout)["instances"]
            if any(value["instance_id"] == instance for value in values):
                return process
        assert process.poll() is None, "native host exited before registration"
        assert time.monotonic() < deadline, "native host registration timed out"
        time.sleep(0.02)


def stop_host(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)


def generate_recordings(project):
    instance = f"replay-source-{os.getpid()}"
    process = start_host(HEADLESS, ["--serve", "--allow-mutation",
                                   "--diagnostics", "never"], instance, project)
    try:
        call(instance, project, "step", 8)
        short = call(instance, project, "query", "recording")["response"]["value"]
        call(instance, project, "step", 3592)
        maximum = call(instance, project, "query", "recording")["response"]["value"]
        assert short["recorded_ticks"] == 8
        assert maximum["recorded_ticks"] == 3600 and not maximum["truncated"]
        return short, maximum
    finally:
        stop_host(process)


def session_state(instance, project):
    status = call(instance, project, "status")
    return {
        "status": {
            "observed_frame": status["observed_frame"],
            "state_revision": status["state_revision"],
            "value": status["response"],
        },
        "save": call(instance, project, "query", "save")["response"]["value"],
        "replay": call(instance, project, "query", "arena_state")["response"]["value"]["replay"],
        "checksum": call(instance, project, "capture")["response"]["checksum"],
    }


def measure_control(instance, project, recording, repeats, rejected=False):
    argument_path = project / "load-replay.json"
    argument_path.write_text(json.dumps({"recording": recording}, separators=(",", ":")))
    samples = []
    for _ in range(repeats):
        before = session_state(instance, project) if rejected else None
        load_command = command(instance, project, "invoke", "load_replay",
                               "--arguments-file", argument_path)
        started = time.monotonic_ns()
        load = subprocess.Popen(load_command, cwd=GAME, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True)
        # Give the load request a chance to enter the runtime queue. Record the
        # overlap explicitly instead of assuming the scheduling race was won.
        time.sleep(0.001)
        overlapping = load.poll() is None
        probe_started = time.monotonic_ns()
        status = call(instance, project, "status")
        probe_ms = (time.monotonic_ns() - probe_started) / 1_000_000
        stdout, stderr = load.communicate(timeout=10)
        load_ms = (time.monotonic_ns() - started) / 1_000_000
        response = json.loads(stdout)
        assert (response["status"] == "failure") == rejected, (response, stderr)
        unchanged = None
        if rejected:
            unchanged = session_state(instance, project) == before
            assert unchanged, "final-verification rejection changed the native session"
        samples.append({
            "load_round_trip_ms": round(load_ms, 6),
            "concurrent_status_ms": round(probe_ms, 6),
            "load_running_when_status_started": overlapping,
            "status_frame": status["observed_frame"],
            "result": response["status"],
            "error_code": response.get("error", {}).get("code"),
            "rejected_session_unchanged": unchanged,
        })
    return samples


def measure_file(path, repeats, success):
    samples = []
    for _ in range(repeats):
        started = time.monotonic_ns()
        result = run([str(REPLAY), str(path)], capture_output=True, text=True)
        elapsed = (time.monotonic_ns() - started) / 1_000_000
        assert (result.returncode == 0) == success, (result.stdout, result.stderr)
        samples.append(round(elapsed, 6))
    return samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=7)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")
    for artifact in [CLI, HEADLESS, PLAYER, REPLAY]:
        if not artifact.is_file():
            parser.error(f"missing optimized artifact: {artifact}")

    with tempfile.TemporaryDirectory(prefix="titan-replay-import-") as directory:
        project = Path(directory).resolve()
        short, maximum = generate_recordings(project)
        compact = json.dumps(maximum, separators=(",", ":"))
        padding = MAX_BYTES - len(compact.encode())
        assert padding >= 0
        exact_limit = " " * padding + compact
        assert len(exact_limit.encode()) == MAX_BYTES
        mismatch = {**maximum, "final_checksum": "0000000000000000"}
        files = {}
        for name, contents in {
            "short": json.dumps(short, separators=(",", ":")),
            "max_ticks": compact,
            "max_bytes": exact_limit,
            "final_mismatch": json.dumps(mismatch, separators=(",", ":")),
        }.items():
            path = project / f"{name}.json"
            path.write_text(contents)
            files[name] = path

        instance = f"replay-import-{os.getpid()}"
        process = start_host(PLAYER, ["--inspect", "--allow-control"], instance, project)
        try:
            call(instance, project, "invoke", "pause")
            control = {
                "short_valid": measure_control(instance, project, short, args.repeats),
                "max_ticks_valid": measure_control(instance, project, maximum, args.repeats),
                "max_ticks_final_mismatch": measure_control(
                    instance, project, mismatch, args.repeats, rejected=True),
            }
        finally:
            stop_host(process)

        report = {
            "schema_version": 1,
            "measured_at_utc": datetime.now(timezone.utc).isoformat(),
            "revision": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
            "working_tree_dirty": bool(subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=REPO)),
            "environment": {
                "platform": platform.platform(),
                "machine": platform.machine(),
                "logical_cpus": os.cpu_count(),
                "python": platform.python_version(),
                "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                "profile": "release",
                "host": "native GPU player",
            },
            "recordings": {
                "short": {"ticks": 8, "bytes": files["short"].stat().st_size},
                "max_ticks": {"ticks": 3600, "bytes": files["max_ticks"].stat().st_size},
                "max_bytes": {"ticks": 3600, "bytes": files["max_bytes"].stat().st_size,
                              "padding": "leading JSON whitespace"},
            },
            "measurement_scope": {
                "control": "CLI process round trip; concurrent status is launched 1 ms after load and overlap is recorded per sample",
                "file": "fresh replay verifier process including launch, JSON read/parse, complete replay, final snapshot and software-pixel comparison",
            },
            "control": control,
            "file_verification_ms": {
                "short_valid": measure_file(files["short"], args.repeats, True),
                "max_ticks_valid": measure_file(files["max_ticks"], args.repeats, True),
                "max_bytes_valid": measure_file(files["max_bytes"], args.repeats, True),
                "max_ticks_final_mismatch": measure_file(
                    files["final_mismatch"], args.repeats, False),
            },
        }
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
