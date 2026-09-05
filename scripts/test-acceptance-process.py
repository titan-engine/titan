#!/usr/bin/env python3
"""Exercise deadline/descendant cleanup without building an engine."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import acceptance_process as processes


class AcceptanceProcessTests(unittest.TestCase):
    def test_success_and_nonzero(self):
        self.assertEqual(processes.check_output([sys.executable, "-c", "print('ok')"], text=True), "ok\n")
        with self.assertRaises(subprocess.CalledProcessError):
            processes.run([sys.executable, "-c", "raise SystemExit(7)"], check=True)

    def test_configured_phases_and_invalid_limits(self):
        with patch.dict(os.environ, {"TITAN_RUNTIME_TIMEOUT_SECONDS": "0.15", "TITAN_BUILD_TIMEOUT_SECONDS": "0.25"}):
            self.assertEqual(processes.timeout_seconds("runtime"), 0.15)
            self.assertEqual(processes.timeout_seconds("build"), 0.25)
            for phase in ("runtime", "build"):
                started = time.monotonic()
                with self.assertRaisesRegex(subprocess.TimeoutExpired, f"acceptance {phase} phase"):
                    processes.run([sys.executable, "-c", "import time; time.sleep(30)"], phase=phase)
                self.assertLess(time.monotonic() - started, 5)
        for value in ("0", "-1", "nan", "inf", "invalid"):
            with patch.dict(os.environ, {"TITAN_RUNTIME_TIMEOUT_SECONDS": value}):
                with self.assertRaisesRegex(ValueError, "finite positive"):
                    processes.timeout_seconds()

    def test_pipe_holding_descendant_after_leader_exit(self):
        # The leader exits successfully but its child inherits both output pipes.
        script = "import subprocess,sys; subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)']); print('leader exited',flush=True)"
        started = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired) as caught:
            processes.run([sys.executable, "-c", script], capture_output=True, timeout=0.4)
        self.assertLess(time.monotonic() - started, 5)
        self.assertIn(b"leader exited", caught.exception.output)

    def test_descendant_ignoring_term_is_killed(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "alive"
            child = "import signal,time,pathlib; signal.signal(signal.SIGTERM,signal.SIG_IGN); p=pathlib.Path(" + repr(str(marker)) + ");\nwhile True: p.write_text(str(time.monotonic())); time.sleep(.02)"
            script = "import subprocess,sys,time; subprocess.Popen([sys.executable,'-c'," + repr(child) + "]); time.sleep(30)"
            with self.assertRaises(subprocess.TimeoutExpired):
                processes.run([sys.executable, "-c", script], timeout=0.5)
            before = marker.read_text()
            time.sleep(0.15)
            self.assertEqual(marker.read_text(), before)

    def test_watchdog_without_wait(self):
        process = processes.Popen([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.15)
        time.sleep(1.5)
        self.assertIsNotNone(process.returncode)
        with self.assertRaises(subprocess.TimeoutExpired):
            process.poll()
        processes.terminate(process)
        processes.terminate(process)

    def test_registration_cleanup_is_scoped(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            registry = project / "target/titan/instances"
            registry.mkdir(parents=True)
            process = processes.Popen([sys.executable, "-c", "import time; time.sleep(30)"], project=project, instance="test", timeout=0.5)
            base = {"pid": process.pid, "project": str(project), "instance_id": "test", "token": "not-logged"}
            variants = {"owned": base, "other-instance": {**base, "instance_id": "other"}, "other-project": {**base, "project": str(project / "other")}, "other-pid": {**base, "pid": os.getpid()}}
            for name, data in variants.items():
                (registry / f"{name}.json").write_text(json.dumps(data))
            (registry / "malformed.json").write_text("{")
            with self.assertRaises(subprocess.TimeoutExpired):
                process.wait()
            self.assertFalse((registry / "owned.json").exists())
            self.assertEqual({path.stem for path in registry.iterdir()}, {"other-instance", "other-project", "other-pid", "malformed"})

    def test_nested_session_obeys_inherited_deadline(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "alive"
            child = "import signal,time,pathlib; signal.signal(signal.SIGTERM,signal.SIG_IGN); p=pathlib.Path(" + repr(str(marker)) + ");\nwhile True: p.write_text(str(time.monotonic())); time.sleep(.02)"
            nested = "import sys; sys.path.insert(0," + repr(str(Path(__file__).resolve().parent)) + "); import acceptance_process as p; p.run([sys.executable,'-c'," + repr(child) + "])"
            result = processes.run([sys.executable, "-c", nested], capture_output=True, timeout=4)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"acceptance runtime phase", result.stderr)
            before = marker.read_text()
            time.sleep(0.15)
            self.assertEqual(marker.read_text(), before)

    def test_early_parent_timeout_cleans_nested_session(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "alive"
            child = "import signal,time,pathlib; signal.signal(signal.SIGTERM,signal.SIG_IGN); p=pathlib.Path(" + repr(str(marker)) + ");\nwhile True: p.write_text(str(time.monotonic())); time.sleep(.02)"
            setup = "import sys,time; sys.path.insert(0," + repr(str(Path(__file__).resolve().parent)) + "); import acceptance_process as p; "
            middle = setup + "p.Popen([sys.executable,'-c'," + repr(child) + "]); time.sleep(30)"
            nested = setup + "p.Popen([sys.executable,'-c'," + repr(middle) + "]); time.sleep(30)"
            process = processes.Popen([sys.executable, "-c", nested])
            with self.assertRaises(subprocess.TimeoutExpired):
                process.wait(timeout=0.5)
            before = marker.read_text()
            time.sleep(0.15)
            self.assertEqual(marker.read_text(), before)

    def test_graceful_shutdown_preserves_registration_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            registry = project / "target/titan/instances"
            registry.mkdir(parents=True)
            process = processes.Popen([sys.executable, "-c", "import time; time.sleep(30)"], project=project, instance="test")
            path = registry / "owned.json"
            path.write_text(json.dumps({"pid": process.pid, "project": str(project), "instance_id": "test"}))
            try:
                processes.graceful_shutdown(process)
                self.assertTrue(path.exists(), "helper must not hide missing host cleanup")
            finally:
                processes.terminate(process)
            self.assertFalse(path.exists())

    def test_graceful_shutdown_timeout_cleans_registration(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            registry = project / "target/titan/instances"
            registry.mkdir(parents=True)
            process = processes.Popen([sys.executable, "-c", "import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print('ready',flush=True); time.sleep(30)"], project=project, instance="test", timeout=0.5, stdout=subprocess.PIPE, text=True)
            path = registry / "owned.json"
            path.write_text(json.dumps({"pid": process.pid, "project": str(project), "instance_id": "test"}))
            try:
                self.assertEqual(process.stdout.readline().strip(), "ready")
                with self.assertRaises(subprocess.TimeoutExpired):
                    processes.graceful_shutdown(process)
                self.assertFalse(path.exists())
            finally:
                processes.terminate(process)
                process.stdout.close()

    def test_explicit_wait_timeout_and_log_passthrough(self):
        with tempfile.TemporaryFile(mode="w+") as log:
            process = processes.Popen([sys.executable, "-c", "import time; print('started',flush=True); time.sleep(30)"], stdout=log, stderr=log)
            with self.assertRaises(subprocess.TimeoutExpired):
                process.wait(timeout=0.2)
            log.seek(0)
            self.assertIn("started", log.read())
            self.assertIsNotNone(process.returncode)

    def test_cleanup_after_leader_already_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "alive"
            child = "import time,pathlib; p=pathlib.Path(" + repr(str(marker)) + ");\nwhile True: p.write_text(str(time.monotonic())); time.sleep(.02)"
            script = "import subprocess,sys,time; subprocess.Popen([sys.executable,'-c'," + repr(child) + "]); time.sleep(.1)"
            process = processes.Popen([sys.executable, "-c", script])
            process.wait()
            before = marker.read_text()
            time.sleep(0.15)
            self.assertEqual(marker.read_text(), before)


if __name__ == "__main__":
    unittest.main()
