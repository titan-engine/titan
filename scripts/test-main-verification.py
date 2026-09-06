#!/usr/bin/env python3
"""Exercise the inline main-verification policy without a YAML dependency."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = (ROOT / '.github/workflows/main-verification.yml').read_text()


def step_script(name):
    """Read one literal run block from this workflow's fixed step indentation."""
    lines = WORKFLOW.splitlines()
    start = lines.index('      - name: ' + name)
    for index in range(start + 1, len(lines)):
        if lines[index] == '        run: |':
            body = []
            for line in lines[index + 1:]:
                if line and not line.startswith('          '):
                    break
                body.append(line[10:])
            return '\n'.join(body) + '\n'
        if lines[index].startswith('      - '):
            break
    raise AssertionError('Missing literal run block: ' + name)


class MainVerificationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git('init', '--quiet')
        self.git('config', 'user.name', 'CI policy test')
        self.git('config', 'user.email', 'ci-policy@example.invalid')
        self.git('commit', '--quiet', '--allow-empty', '-m', 'baseline')
        self.before = self.git('rev-parse', 'HEAD').strip()

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.root, text=True,
                                       stderr=subprocess.STDOUT, timeout=10)

    def invoke(self, name, **values):
        output = self.root / 'output'
        summary = self.root / 'summary'
        output.write_text('')
        summary.write_text('')
        environment = dict(os.environ, GITHUB_OUTPUT=str(output),
                           GITHUB_STEP_SUMMARY=str(summary),
                           GITHUB_SHA=self.git('rev-parse', 'HEAD').strip(),
                           GITHUB_SERVER_URL='https://github.com',
                           GITHUB_REPOSITORY='titan-engine/titan', GITHUB_RUN_ID='123',
                           GITHUB_EVENT_NAME='push', BEFORE_SHA=self.before)
        environment.update(values)
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', step_script(name)],
                                cwd=self.root, env=environment, capture_output=True,
                                text=True, timeout=10)
        return result, output.read_text(), summary.read_text()

    def warm(self, **values):
        return self.invoke('Select full verification for cache input changes', **values)

    def test_manual_schedule_and_unknown_previous_revision_warm(self):
        for values in ({'GITHUB_EVENT_NAME': 'workflow_dispatch'},
                       {'GITHUB_EVENT_NAME': 'schedule'}, {'BEFORE_SHA': '0' * 40},
                       {'BEFORE_SHA': ''}, {'BEFORE_SHA': 'malformed'}):
            with self.subTest(values=values):
                result, output, summary = self.warm(**values)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, 'full=true\n')
                self.assertIn('Full CI selected', summary)

    def test_cache_inputs_warm_but_source_only_changes_allow_lookup(self):
        for path, expected in (
                ('crates/example/src/lib.rs', False), ('docs/guide.md', False),
                ('Cargo.toml', True), ('games/example/Cargo.lock', True),
                ('rust-toolchain.toml', True), ('.python-version', True),
                ('.node-version', True), ('.github/workflows/ci.yml', True),
                ('.github/actions/ci-setup/action.yml', True), ('scripts/ci-cache.py', True)):
            with self.subTest(path=path):
                self.before = self.git('rev-parse', 'HEAD').strip()
                target = self.root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text('changed\n')
                self.git('add', path)
                self.git('commit', '--quiet', '-m', 'change input')
                result, output, _ = self.warm()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, f'full={str(expected).lower()}\n')

    def test_failed_git_lookup_does_not_emit_reuse_decision(self):
        result, output, _ = self.warm(BEFORE_SHA='f' * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output, '')

    def test_successful_reuse_links_exact_revision(self):
        queue = 'https://github.com/titan-engine/titan/actions/runs/456'
        result, _, summary = self.invoke('Record accepted evidence or fail',
                                        DECISION_RESULT='success', REUSE='true',
                                        FULL_RESULT='skipped', QUEUE_RUN=queue)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(self.before, summary)
        self.assertIn(queue, summary)
        self.assertIn('Trusted merge-queue CI', summary)

    def test_full_success_accepts_failed_decision(self):
        result, _, summary = self.invoke('Record accepted evidence or fail',
                                        DECISION_RESULT='failure', REUSE='',
                                        FULL_RESULT='success', QUEUE_RUN='')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Full main CI', summary)
        self.assertIn(self.before, summary)
        self.assertIn('/actions/runs/123', summary)

    def test_unverified_results_fail_closed(self):
        for decision, reuse, full, queue in (
                ('success', 'false', 'skipped', ''),
                ('success', 'true', 'skipped', ''),
                ('failure', 'true', 'skipped', 'https://example.invalid/run'),
                ('cancelled', 'true', 'skipped', 'https://example.invalid/run'),
                ('success', 'true', 'failure', 'https://example.invalid/run'),
                ('success', 'true', 'cancelled', 'https://example.invalid/run'),
                ('skipped', '', 'skipped', ''), ('', '', '', '')):
            with self.subTest(decision=decision, reuse=reuse, full=full, queue=queue):
                result, _, summary = self.invoke('Record accepted evidence or fail',
                                                DECISION_RESULT=decision, REUSE=reuse,
                                                FULL_RESULT=full, QUEUE_RUN=queue)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('No accepted exact-revision verification succeeded', summary)

    def test_workflow_routes_fallback_and_preserves_integration_triggers(self):
        full = WORKFLOW.split('\n  full:\n', 1)[1].split('\n  verified:\n', 1)[0]
        self.assertIn('    needs: decide\n', full)
        self.assertIn("    if: always() && !cancelled() && (needs.decide.result != 'success' || needs.decide.outputs.reuse != 'true')\n", full)
        self.assertIn('    uses: ./.github/workflows/ci.yml\n', full)
        suite = (ROOT / '.github/workflows/ci.yml').read_text()
        triggers = suite.split('\non:\n', 1)[1].split('\nconcurrency:', 1)[0]
        self.assertNotIn('  push:', triggers)
        for trigger in ('pull_request', 'merge_group', 'workflow_dispatch', 'workflow_call'):
            self.assertIn(f'  {trigger}:', triggers)
        self.assertIn('    types: [checks_requested]', triggers)


if __name__ == '__main__':
    unittest.main()
