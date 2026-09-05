#!/usr/bin/env python3
"""Verify actual process containment and failure evidence without invoking Cargo."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("png_fuzz", Path(__file__).with_name("fuzz-png.py"))
fuzz = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fuzz)


class Containment(unittest.TestCase):
    def run_child(self, code, timeout=5):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "run.log"
            fuzz.run_bounded([sys.executable, "-c", code], log, timeout=timeout)
            return log.read_text()

    def test_resource_limits_installed(self):
        self.assertIn("limits verified", self.run_child(
            "import resource,sys; "
            "assert resource.getrlimit(resource.RLIMIT_CPU) == (30,30); "
            "assert resource.getrlimit(resource.RLIMIT_FSIZE) == (8388608,8388608); "
            "assert resource.getrlimit(resource.RLIMIT_CORE) == (0,0); "
            "assert sys.platform != 'linux' or resource.getrlimit(resource.RLIMIT_AS) == (1073741824,1073741824); "
            "print('limits verified')"))

    def test_hang_is_stopped(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            self.run_child("import time; time.sleep(30)", timeout=0.2)

    def test_failed_child_retains_output(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "run.log"
            with self.assertRaisesRegex(RuntimeError, "status 7"):
                fuzz.run_bounded([sys.executable, "-c", "print('failure case'); raise SystemExit(7)"], log)
            self.assertIn("failure case", log.read_text())

    @unittest.skipUnless(sys.platform == "linux", "Linux hard address-space limit")
    def test_memory_allocation_is_bounded(self):
        with self.assertRaises(RuntimeError):
            self.run_child("x = bytearray(2 * 1024**3)")


if __name__ == "__main__":
    unittest.main()
