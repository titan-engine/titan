#!/usr/bin/env python3
"""Exercise timing boundaries and missing evidence without network access."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('measure_ci', Path(__file__).with_name('measure-ci.py'))
measurement = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measurement)


class MeasurementTests(unittest.TestCase):
    def fixture(self):
        run = dict(status='completed', run_started_at='2026-09-06T10:00:00Z',
                   html_url='https://example.test/run', event='pull_request',
                   head_sha='abc', run_attempt=2, conclusion='success')
        jobs = [dict(name=name, status='completed', conclusion='success',
                     started_at='2026-09-06T10:00:10Z', completed_at='2026-09-06T10:01:00Z',
                     steps=[]) for name in measurement.REQUIRED]
        return run, jobs

    def test_parallel_wall_time_is_not_sum_and_unknown_size_is_not_zero(self):
        run, jobs = self.fixture()
        result = measurement.summarize(run, jobs, '')
        self.assertEqual(result['required_check_seconds'], 60)
        self.assertEqual(result['runner_minutes'], 2.5)
        self.assertEqual(result['initial_runner_wait_seconds'], 10)
        self.assertIsNone(result['jobs'][0]['restored_archive_bytes'])

    def test_cache_steps_and_logs(self):
        run, jobs = self.fixture()
        jobs[0]['steps'] = [dict(name=name, conclusion='success',
                                 started_at='2026-09-06T10:00:10Z', completed_at='2026-09-06T10:00:15Z')
                            for name in ('Restore workload cache', 'Save workload cache',
                                         'Verify generated asset cache across processes')]
        result = measurement.summarize(run, jobs, 'Native checks\tstep\tCache Size: ~1 MB (123 B)\n')
        row = result['jobs'][0]
        self.assertEqual(row['cache_restore_seconds'], 5)
        self.assertEqual(row['cache_save_seconds'], 5)
        self.assertEqual(row['restored_archive_bytes'], [123])

    def test_rejects_incomplete_or_missing_gate(self):
        run, jobs = self.fixture()
        with self.assertRaises(ValueError):
            measurement.summarize(run, jobs[:-1], '')
        run['status'] = 'in_progress'
        with self.assertRaises(ValueError):
            measurement.summarize(run, jobs, '')


if __name__ == '__main__':
    unittest.main()
