#!/usr/bin/env python3
"""Check the shared CI budget, failure status and exhausted-step behavior."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

SPEC = importlib.util.spec_from_file_location('ci_deadline', Path(__file__).with_name('ci-deadline.py'))
deadline = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(deadline)


class DeadlineTests(unittest.TestCase):
    def test_budget_counts_previous_steps_and_actions(self):
        self.assertEqual(deadline.remaining(100, 100), 2700)
        self.assertEqual(deadline.remaining(100, 2700), 100)
        self.assertEqual(deadline.remaining(100, 3000), 0)

    def invoke(self, body, started=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'step.sh'
            script.write_text(body)
            environment = dict(os.environ, GITHUB_ENV=str(root / 'env'))
            environment.pop('TITAN_CI_STARTED_AT', None)
            if started is not None:
                environment['TITAN_CI_STARTED_AT'] = str(started)
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('ci-deadline.py')), str(script)],
                                    env=environment, capture_output=True, text=True, timeout=10)
            return result, (root / 'env').read_text() if (root / 'env').exists() else ''

    def test_original_exit_status_and_persisted_start(self):
        result, environment = self.invoke('exit 7\n')
        self.assertEqual(result.returncode, 7)
        self.assertIn('TITAN_CI_STARTED_AT=', environment)

    def test_exhausted_budget_does_not_launch_step(self):
        result, _ = self.invoke('echo SHOULD_NOT_RUN\n', time.time() - 2800)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('SHOULD_NOT_RUN', result.stdout)
        self.assertIn('headroom', result.stderr)


if __name__ == '__main__':
    unittest.main()
