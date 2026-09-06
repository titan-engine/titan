#!/usr/bin/env python3
"""Trust-boundary regression tests; no network or third-party dependencies."""
import base64
import copy
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import ci_queue_evidence as evidence

SHA = 'a' * 40
REPOSITORY = 'titan-engine/titan'
WORKFLOW = b'''name: CI
jobs:
  native:
    name: Native checks
    steps:
      - name: Test native
        run: cargo test
  wasm:
    name: WebAssembly core check
    steps:
      - name: Test wasm
        run: cargo check
  macos-bundles:
    name: macOS development app bundles
    steps:
      - name: Test macos
        run: python3 test.py
'''


class FakeAPI:
    def __init__(self):
        self.repo = {'id': 42, 'full_name': REPOSITORY}
        self.workflow = {'id': 7, 'path': evidence.WORKFLOW_PATH}
        self.revision = {'type': 'file', 'encoding': 'base64',
                         'content': base64.b64encode(WORKFLOW).decode()}
        self.run = {'id': 8, 'workflow_id': 7, 'path': evidence.WORKFLOW_PATH,
                    'head_sha': SHA, 'event': 'merge_group', 'status': 'completed',
                    'conclusion': 'success', 'run_attempt': 1, 'updated_at': 'stable',
                    'repository': self.repo.copy(), 'head_repository': self.repo.copy()}
        self.candidates = [{'id': 8}]
        self.jobs = []
        contract = evidence.required_steps(WORKFLOW)
        contract['CI revision'] = ['Verify CI workflow revision ' + SHA]
        for index, (name, steps) in enumerate(contract.items()):
            self.jobs.append({'id': index + 1, 'run_id': 8, 'head_sha': SHA,
                              'name': name, 'status': 'completed', 'conclusion': 'success',
                              'steps': [{'name': name, 'status': 'completed',
                                         'conclusion': 'success'} for name in steps]})
        self.reads = 0
        self.fresh_change = {}
        self.failure = None
        self.calls = []

    def get(self, path, **params):
        self.calls.append((path, params))
        if self.failure and self.failure in path:
            raise OSError('API unavailable')
        if path.endswith('/titan'):
            return copy.deepcopy(self.repo)
        if '/contents/' in path:
            assert params == {'ref': SHA}
            return copy.deepcopy(self.revision)
        if path.endswith('/workflows/ci.yml'):
            return copy.deepcopy(self.workflow)
        if path.endswith('/workflows/7/runs'):
            assert params['event'] == 'merge_group' and params['head_sha'] == SHA
            return {'total_count': len(self.candidates), 'workflow_runs': copy.deepcopy(self.candidates)}
        if path.endswith('/attempts/1/jobs'):
            return {'total_count': len(self.jobs), 'jobs': copy.deepcopy(self.jobs)}
        if path.endswith('/runs/8'):
            self.reads += 1
            run = copy.deepcopy(self.run)
            if self.reads > 1:
                run.update(self.fresh_change)
            return run
        raise AssertionError(path)


class EvidenceTests(unittest.TestCase):
    def match(self, api=None):
        return evidence.find_evidence(api or FakeAPI(), REPOSITORY, SHA, WORKFLOW)

    def test_exact_match(self):
        api = FakeAPI()
        result = self.match(api)
        self.assertEqual(result['reuse'], 'true')
        self.assertEqual(result['head_sha'], SHA)
        self.assertTrue(result['run_url'].endswith('/8/attempts/1'))
        self.assertEqual(api.reads, 2)

    def test_run_identity_and_conclusion_must_match(self):
        for field, values in {
            'id': [9, None], 'workflow_id': [9, None], 'path': ['other.yml', None],
            'head_sha': ['b' * 40, SHA[:12], None],
            'event': ['push', 'pull_request', 'workflow_dispatch', None],
            'status': ['queued', 'in_progress', None],
            'conclusion': ['failure', 'cancelled', 'skipped', 'neutral', None],
            'run_attempt': [0, None, True],
        }.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    api = FakeAPI()
                    api.run[field] = value
                    with self.assertRaises(evidence.EvidenceUnavailable):
                        self.match(api)

    def test_same_repository_required_including_fork_identity(self):
        for field in ('repository', 'head_repository'):
            for value in (None, {'id': 43, 'full_name': REPOSITORY},
                          {'id': 42, 'full_name': 'fork/titan'}):
                with self.subTest(field=field, value=value):
                    api = FakeAPI()
                    api.run[field] = value
                    with self.assertRaises(evidence.EvidenceUnavailable):
                        self.match(api)

    def test_missing_or_ambiguous_candidates(self):
        for candidates in ([], [{'id': 8}, {'id': 9}], [{'id': None}]):
            api = FakeAPI()
            api.candidates = candidates
            with self.assertRaises(evidence.EvidenceUnavailable):
                self.match(api)

    def test_workflow_identity_and_revision(self):
        api = FakeAPI()
        api.workflow['path'] = '.github/workflows/other.yml'
        with self.assertRaises(evidence.EvidenceUnavailable):
            self.match(api)
        api = FakeAPI()
        api.revision['content'] = base64.b64encode(WORKFLOW + b'# changed').decode()
        with self.assertRaises(evidence.EvidenceUnavailable):
            self.match(api)

    def test_required_gates_and_substantive_steps(self):
        for index in range(4):
            for change in ('missing', 'duplicate', 'failed', 'skipped-step', 'missing-step', 'wrong-sha'):
                with self.subTest(index=index, change=change):
                    api = FakeAPI()
                    if change == 'missing':
                        api.jobs.pop(index)
                    elif change == 'duplicate':
                        duplicate = copy.deepcopy(api.jobs[index])
                        duplicate['id'] = 99
                        api.jobs.append(duplicate)
                    elif change == 'failed':
                        api.jobs[index]['conclusion'] = 'failure'
                    elif change == 'skipped-step':
                        api.jobs[index]['steps'][0]['conclusion'] = 'skipped'
                    elif change == 'missing-step':
                        api.jobs[index]['steps'] = []
                    else:
                        api.jobs[index]['head_sha'] = 'b' * 40
                    with self.assertRaises(evidence.EvidenceUnavailable):
                        self.match(api)

    def test_rerun_started_during_lookup(self):
        for change in ({'run_attempt': 2}, {'status': 'in_progress'}, {'updated_at': 'new'}):
            api = FakeAPI()
            api.fresh_change = change
            with self.assertRaises(evidence.EvidenceUnavailable):
                self.match(api)

    def test_api_failures_never_create_evidence(self):
        for endpoint in ('/titan', '/workflows/ci.yml', '/contents/', '/workflows/7/runs',
                         '/runs/8', '/attempts/1/jobs'):
            api = FakeAPI()
            api.failure = endpoint
            with self.assertRaises(OSError):
                self.match(api)

    def test_pagination_complete_and_fail_closed(self):
        class Paged:
            def get(self, path, **params):
                start = (params['page'] - 1) * 100
                return {'total_count': 101, 'jobs': list(range(start, min(start + 100, 101)))}
        self.assertEqual(len(evidence.pages(Paged(), '/jobs', 'jobs')), 101)
        with patch.object(evidence, 'MAX_PAGES', 1):
            with self.assertRaises(evidence.EvidenceUnavailable):
                evidence.pages(Paged(), '/jobs', 'jobs')
        class Partial:
            def get(self, path, **params):
                return {'total_count': 101, 'jobs': [1]}
        with self.assertRaises(evidence.EvidenceUnavailable):
            evidence.pages(Partial(), '/jobs', 'jobs')

    def test_event_dependent_or_conditional_verification_falls_back(self):
        for workflow in (
            WORKFLOW.replace(b'    name: Native checks', b'    if: true\n    name: Native checks'),
            WORKFLOW.replace(b'        run: cargo test', b'        if: true\n        run: cargo test'),
            WORKFLOW.replace(b'        run: cargo test', b'        run: cargo test\n        if: true'),
            WORKFLOW.replace(b'cargo test', b'echo $GITHUB_EVENT_NAME'),
            WORKFLOW.replace(b'cargo test', b'echo ${{ github.ref }}'),
            WORKFLOW.replace(b'  native:', b'  native-aggregate:'),
        ):
            with self.subTest(workflow=workflow):
                with self.assertRaises(evidence.EvidenceUnavailable):
                    evidence.required_steps(workflow)

    def test_actual_workflow_has_all_contracts(self):
        workflow = Path(__file__).resolve().parents[1].joinpath(evidence.WORKFLOW_PATH).read_bytes()
        self.assertEqual(set(evidence.required_steps(workflow)), set(evidence.REQUIRED_JOBS.values()))

    def test_cli_defaults_to_full_ci_without_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'output'
            summary = Path(directory) / 'summary'
            env = dict(os.environ, GH_TOKEN='', GITHUB_OUTPUT=str(output),
                       GITHUB_STEP_SUMMARY=str(summary), GITHUB_EVENT_NAME='push',
                       GITHUB_REF='refs/heads/main')
            result = subprocess.run([sys.executable, str(Path(evidence.__file__))], env=env,
                                    capture_output=True, text=True, check=True)
            self.assertIn('reuse=false', result.stdout)
            self.assertIn('reuse=false', output.read_text())
            self.assertIn('Full required CI will run', summary.read_text())


if __name__ == '__main__':
    unittest.main()
