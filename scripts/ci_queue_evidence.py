#!/usr/bin/env python3
"""Fail-closed, exact-revision merge-queue evidence lookup (stdlib only)."""
import base64
import json
import os
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request

WORKFLOW_PATH = '.github/workflows/ci.yml'
REQUIRED_JOBS = {
    'native': 'Native checks',
    'wasm': 'WebAssembly core check',
    'macos-bundles': 'macOS development app bundles',
}
MAX_PAGES = 10


class EvidenceUnavailable(Exception):
    """The lookup cannot prove the complete evidence contract."""


def require(condition, reason):
    if not condition:
        raise EvidenceUnavailable(reason)


class GitHubAPI:
    def __init__(self, token):
        self.token = token

    def get(self, path, **params):
        url = 'https://api.github.com' + path
        if params:
            url += '?' + urllib.parse.urlencode(params)
        request = urllib.request.Request(url, headers={
            'Accept': 'application/vnd.github+json',
            'Authorization': 'Bearer ' + self.token,
            'X-GitHub-Api-Version': '2022-11-28',
            'User-Agent': 'titan-ci-queue-evidence',
        })
        with urllib.request.urlopen(request, timeout=15) as response:
            return json.load(response)


def pages(api, path, key, **params):
    results = []
    total = None
    for page in range(1, MAX_PAGES + 1):
        data = api.get(path, per_page=100, page=page, **params)
        count = data.get('total_count')
        batch = data.get(key)
        require(type(count) is int and count >= 0 and isinstance(batch, list),
                'Malformed paginated evidence')
        require(total is None or total == count, 'Evidence changed during pagination')
        total = count
        results.extend(batch)
        require(len(results) <= total, 'Inconsistent pagination')
        if len(results) == total:
            return results
        require(len(batch) == 100, 'Incomplete evidence pagination')
    raise EvidenceUnavailable('Evidence pagination limit reached')


def required_steps(workflow):
    """Read only the deliberately narrow, block-style gate layout we support.

    This is not a YAML parser. Changed layouts (including reusable workflows or
    aggregate gates) must update the evidence contract; unknown layouts fall back.
    All named run steps must have executed, preventing a skipped heavy workload
    from being accepted behind a successful job conclusion.
    """
    steps = {job: [] for job in REQUIRED_JOBS}
    current = None
    name = None
    conditional = False
    for line in workflow.decode('utf-8').splitlines():
        match = re.fullmatch(r'  ([a-zA-Z0-9_-]+):', line)
        if match:
            current = match[1] if match[1] in steps else None
            name = None
        if current is None:
            continue
        require(not line.startswith('    if:'), 'Conditional required gate is unsupported')
        require(not re.search(r'github\.(event|ref)|GITHUB_(EVENT|REF)', line),
                'Event-dependent required verification is unsupported')
        match = re.fullmatch(r'      - name: ([^\'"{}]+)', line)
        if match:
            name = match[1]
            conditional = False
        elif line.startswith('      - '):
            name = None
            conditional = False
        elif line.startswith('        if:'):
            require(name not in steps[current], 'Conditional verification step is unsupported')
            conditional = True
        elif line.startswith('        run:'):
            require(name is not None, 'Unsupported unnamed verification step')
            require(not conditional, 'Conditional verification step is unsupported')
            steps[current].append(name)
    require(all(steps.values()), 'Unsupported required-gate workflow layout')
    require(all(len(names) == len(set(names)) for names in steps.values()),
            'Ambiguous verification step names')
    return {REQUIRED_JOBS[job]: names for job, names in steps.items()}


def find_evidence(api, repository, sha, workflow):
    require(re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository),
            'Invalid repository identity')
    require(re.fullmatch(r'[0-9a-f]{40}', sha), 'Expected a full commit SHA')
    prefix = '/repos/' + repository
    repo = api.get(prefix)
    require(repo.get('full_name') == repository and type(repo.get('id')) is int,
            'Repository identity mismatch')
    workflow_info = api.get(prefix + '/actions/workflows/ci.yml')
    workflow_id = workflow_info.get('id')
    require(type(workflow_id) is int and workflow_info.get('path') == WORKFLOW_PATH,
            'CI workflow identity mismatch')
    revision = api.get(prefix + '/contents/' + WORKFLOW_PATH, ref=sha)
    require(revision.get('encoding') == 'base64' and revision.get('type') == 'file',
            'Workflow revision unavailable')
    require(base64.b64decode(revision['content']) == workflow,
            'Workflow revision differs from checkout')
    contract = required_steps(workflow)
    contract['CI revision'] = ['Verify CI workflow revision ' + sha]
    candidates = pages(api, prefix + f'/actions/workflows/{workflow_id}/runs',
                       'workflow_runs', event='merge_group', head_sha=sha)
    require(len(candidates) == 1, 'Missing or ambiguous exact-SHA queue evidence')
    run_id = candidates[0].get('id')
    require(type(run_id) is int and run_id > 0, 'Invalid queue run identity')
    run_path = prefix + f'/actions/runs/{run_id}'

    def validate(run):
        require(run.get('id') == run_id and run.get('workflow_id') == workflow_id
                and run.get('path') == WORKFLOW_PATH, 'Queue workflow identity mismatch')
        require(run.get('head_sha') == sha, 'Queue SHA mismatch')
        require(run.get('event') == 'merge_group', 'Evidence is not a merge-group run')
        require(run.get('status') == 'completed' and run.get('conclusion') == 'success',
                'Queue run is not completed successfully')
        for field in ('repository', 'head_repository'):
            value = run.get(field) or {}
            require(value.get('id') == repo['id'] and value.get('full_name') == repository,
                    'Queue repository mismatch')
        attempt = run.get('run_attempt')
        require(type(attempt) is int and attempt > 0, 'Invalid queue run attempt')
        return attempt

    run = api.get(run_path)
    attempt = validate(run)
    jobs = pages(api, run_path + f'/attempts/{attempt}/jobs', 'jobs')
    require(len({job.get('id') for job in jobs}) == len(jobs), 'Duplicate job evidence')
    for name, expected_steps in contract.items():
        matches = [job for job in jobs if job.get('name') == name]
        require(len(matches) == 1, 'Missing or ambiguous required gate: ' + name)
        job = matches[0]
        require(job.get('run_id') == run_id and job.get('head_sha') == sha,
                'Required gate revision mismatch: ' + name)
        require(job.get('status') == 'completed' and job.get('conclusion') == 'success',
                'Required gate did not succeed: ' + name)
        steps = job.get('steps')
        require(isinstance(steps, list), 'Missing verification steps: ' + name)
        for expected in expected_steps:
            matches = [step for step in steps if step.get('name') == expected]
            require(len(matches) == 1 and matches[0].get('status') == 'completed'
                    and matches[0].get('conclusion') == 'success',
                    'Verification step did not execute successfully: ' + expected)
    fresh = api.get(run_path)
    require(validate(fresh) == attempt and fresh.get('updated_at') == run.get('updated_at'),
            'Queue run changed during lookup')
    return {'reuse': 'true', 'run_id': str(run_id), 'run_attempt': str(attempt),
            'head_sha': sha, 'run_url': f'https://github.com/{repository}/actions/runs/{run_id}/attempts/{attempt}',
            'reason': 'Complete successful merge-group verification for the exact main SHA'}


def main():
    result = {'reuse': 'false', 'reason': 'Queue evidence unavailable; full CI required'}
    try:
        require(os.environ.get('GITHUB_EVENT_NAME') == 'push'
                and os.environ.get('GITHUB_REF') == 'refs/heads/main',
                'Only main push runs may reuse queue evidence')
        token = os.environ.get('GH_TOKEN', '')
        require(bool(token), 'Actions read token unavailable')
        workflow = Path(__file__).resolve().parents[1].joinpath(WORKFLOW_PATH).read_bytes()
        result = find_evidence(GitHubAPI(token), os.environ.get('GITHUB_REPOSITORY', ''),
                               os.environ.get('GITHUB_SHA', ''), workflow)
    except EvidenceUnavailable as error:
        result['reason'] = str(error)
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        # Never include exception bodies, response content or token-bearing URLs.
        result['reason'] = 'Evidence lookup failed; full CI required'
    output = '\n'.join(f'{key}={value}' for key, value in result.items()) + '\n'
    print(output, end='')
    if os.environ.get('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
            stream.write(output)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        summary = 'Queue verification decision: ' + result['reason'] + '.\n'
        if result['reuse'] == 'true':
            summary += f"\nVerified `{result['head_sha']}` with [queue run {result['run_id']}, attempt {result['run_attempt']}]({result['run_url']}).\n"
        else:
            summary += '\nFull required CI will run.\n'
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a', encoding='utf-8') as stream:
            stream.write(summary)


if __name__ == '__main__':
    main()
