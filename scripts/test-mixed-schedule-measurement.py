#!/usr/bin/env python3
"""Check mixed-schedule fresh-child measurement and failure cleanup."""

import importlib.util
from pathlib import Path
import sys
import unittest


spec = importlib.util.spec_from_file_location(
    'measure_mixed_schedules', Path(__file__).with_name('measure-mixed-schedules.py'))
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

    def test_schedule_shapes_are_hoisted_and_must_remain_stable(self):
        shape = {'batch_sizes': [2, 2]}
        sample = {'workload': {'runs': [[
            {'name': 'small_compatible', 'schedule': shape.copy()},
        ], [
            {'name': 'small_compatible', 'schedule': shape.copy()},
        ]]}}
        report = {'schedule_shapes_by_thread_limit': {}}
        measurement.hoist_schedule_shapes(report, sample, 2)
        self.assertEqual(report['schedule_shapes_by_thread_limit']['2'],
                         {'small_compatible': shape})
        self.assertNotIn('schedule', sample['workload']['runs'][0][0])

        changed = {'workload': {'runs': [[
            {'name': 'small_compatible', 'schedule': {'batch_sizes': [4]}},
        ]]}}
        with self.assertRaisesRegex(RuntimeError, 'schedule shape changed'):
            measurement.hoist_schedule_shapes(report, changed, 2)

    def test_checksums_must_match_across_worlds_repeats_and_policies(self):
        report = {'checksums_by_entity_count': {}}
        sample = {'workload': {'runs': [[
            {'name': 'small_compatible', 'checksum': 'abc'},
        ], [
            {'name': 'small_compatible', 'checksum': 'abc'},
        ]]}}
        measurement.verify_checksums(report, sample, 64)
        self.assertEqual(report['checksums_by_entity_count']['64'],
                         {'small_compatible': 'abc'})

        changed = {'workload': {'runs': [[
            {'name': 'small_compatible', 'checksum': 'def'},
        ]]}}
        with self.assertRaisesRegex(RuntimeError, 'checksum changed'):
            measurement.verify_checksums(report, changed, 64)


if __name__ == '__main__':
    unittest.main()
