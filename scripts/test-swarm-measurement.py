#!/usr/bin/env python3
"""Check fresh-child measurement, error propagation and timeout cleanup."""
import importlib.util
from pathlib import Path
import sys
import unittest

spec = importlib.util.spec_from_file_location('measure_swarm', Path(__file__).with_name('measure-swarm.py'))
measurement = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measurement)


@unittest.skipUnless(sys.platform in ('darwin', 'linux'), 'RSS measurement requires macOS/Linux')
class MeasurementTests(unittest.TestCase):
    def test_fresh_process_json_and_rss(self):
        result = measurement.measure([sys.executable, '-c', 'print(\'{"ok": true}\')'], 10)
        self.assertEqual(result['workload'], {'ok': True})
        self.assertGreater(result['peak_process_rss_bytes'], 0)
        self.assertGreater(result['process_wall_seconds'], 0)

    def test_failure_does_not_become_a_sample(self):
        with self.assertRaisesRegex(RuntimeError, 'exited 7'):
            measurement.measure([sys.executable, '-c', 'raise SystemExit(7)'], 10)

    def test_timeout_kills_and_reaps_child(self):
        with self.assertRaises(TimeoutError):
            measurement.measure([sys.executable, '-c', 'import time; time.sleep(30)'], 0.05)

    def test_invalid_json_is_rejected(self):
        with self.assertRaises(ValueError):
            measurement.measure([sys.executable, '-c', 'print("invalid")'], 10)


if __name__ == '__main__':
    unittest.main()
