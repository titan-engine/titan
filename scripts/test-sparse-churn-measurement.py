#!/usr/bin/env python3
"""Check bounded arguments and repeat disagreement without performance thresholds."""
import contextlib
import importlib.util
import io
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('sparse_measurement', Path(__file__).with_name('measure-sparse-churn.py'))
measurement = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measurement)


class RunnerTests(unittest.TestCase):
    def test_rejects_unbounded_work_before_build(self):
        for args in (['--counts', '1000001'], ['--counts', '1000000', '--cycles', '11'],
                     ['--repeats', '0'], ['--cycles', '101'], ['--timeout-seconds', '0'],
                     ['--counts'] + ['1'] * 61):
            with self.subTest(args=args), patch('sys.argv', ['runner', *args]), \
                    contextlib.redirect_stderr(io.StringIO()), \
                    patch.object(measurement.processes, 'run') as build:
                with self.assertRaises(SystemExit) as error:
                    measurement.main()
                self.assertEqual(error.exception.code, 2)
                build.assert_not_called()

    def test_repeats_must_agree(self):
        build = SimpleNamespace(stdout='{"reason":"compiler-artifact","target":{"name":"sparse_churn"},"executable":"fixture"}')
        with patch('sys.argv', ['runner', '--counts', '32', '--distributions', 'dense', '--repeats', '2']), \
                patch.object(measurement.processes, 'run', return_value=build), \
                patch.object(measurement.processes, 'check_output', return_value='test'), \
                patch.object(measurement, 'measure', side_effect=[
                    {'workload': {'correctness': {'checksum': 'a'}}},
                    {'workload': {'correctness': {'checksum': 'b'}}},
                ]), patch.object(measurement.sys, 'platform', 'linux'):
            with self.assertRaisesRegex(RuntimeError, 'repeat disagreement'):
                measurement.main()


if __name__ == '__main__':
    unittest.main()
