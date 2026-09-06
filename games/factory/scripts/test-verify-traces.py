#!/usr/bin/env python3
"""Guard repair replay completeness and recorded-browser comparison failures."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
import sys

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location('verify_traces', Path(__file__).with_name('verify-traces.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RepairChecks(unittest.TestCase):
    def setUp(self):
        self.operations, self.checkpoints = runner.repair_sequence()
        self.expected = json.loads(runner.FIXTURE.read_text())['snapshots']
        # Synthetic conserved boundaries isolate the verifier's rejection paths.
        state = self.expected[0]['state']
        self.result = {'outcomes': [{'state': copy.deepcopy(state)} for _ in self.operations]}
        for (index, _), checkpoint in zip(self.checkpoints, self.expected):
            self.result['outcomes'][index]['state'] = copy.deepcopy(checkpoint['state'])

    def verify(self):
        runner.verify(self.result, self.operations, self.checkpoints, self.expected)

    def test_complete_baseline(self):
        self.verify()

    def test_truncated_outcomes(self):
        self.result['outcomes'].pop()
        with self.assertRaisesRegex(AssertionError, 'operation boundaries'):
            self.verify()

    def test_truncated_browser_baseline(self):
        self.expected.pop()
        with self.assertRaisesRegex(AssertionError, 'browser checkpoints'):
            self.verify()

    def test_reordered_browser_baseline(self):
        self.expected[0], self.expected[1] = self.expected[1], self.expected[0]
        with self.assertRaisesRegex(AssertionError, 'checkpoint order changed'):
            self.verify()

    def test_rejected_operation(self):
        self.result['outcomes'][0]['error'] = 'rejected'
        with self.assertRaises(AssertionError):
            self.verify()

    def test_unconserved_intermediate_boundary(self):
        self.result['outcomes'][0]['state']['extracted'] += 1
        with self.assertRaises(AssertionError):
            self.verify()

    def test_semantic_mismatch(self):
        self.result['outcomes'][self.checkpoints[0][0]]['state']['tick'] += 1
        with self.assertRaisesRegex(AssertionError, 'ore-at-delivery'):
            self.verify()

    def test_ui_fields_do_not_change_semantics(self):
        for row in self.result['outcomes']:
            row['state']['frame'] = 12345
            row['state']['selection'] = 'other'
        self.verify()


if __name__ == '__main__':
    unittest.main()
