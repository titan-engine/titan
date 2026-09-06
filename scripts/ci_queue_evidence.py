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


# Changing the matrix inventory or aggregate layout requires updating this
# contract deliberately, alongside its negative tests.
WORKLOADS = {
    'native': ('native', ['workspace', 'starter', 'collection-room', 'adventure', 'arena', 'factory']),
    'wasm': ('wasm', ['workspace', 'starter', 'collection-room', 'adventure', 'arena', 'factory']),
    'macos-bundles': ('macos', ['workspace', 'bundles', 'adventure', 'arena', 'factory']),
}
CONCURRENCY_EVENT_LINES = {
    '  group: ci-suite-${{ github.event_name }}-${{ github.event.pull_request.number || github.run_id }}',
    "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
}


def required_steps(workflow):
    """Extract the supported block-style matrix contract, without parsing YAML.

    This intentionally recognizes one explicit layout. Unsupported matrices,
    conditions, aliases or job shapes select full CI rather than infer coverage.
    Every named run step applicable to a workload must execute successfully.
    """
    lines = workflow.decode('utf-8').splitlines()
    section = None
    jobs = {}
    current = None
    for line in lines:
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        top = re.match(r'^([a-zA-Z_-]+):', line)
        if top:
            section = top[1]
        event_dependent = re.search(
            r"github\s*(?:\.\s*|\[\s*['\"])(?:event|ref|head_ref|base_ref)"
            r'|GITHUB_(?:EVENT|REF|HEAD_REF|BASE_REF)', line)
        require(not event_dependent or
                (section == 'concurrency' and line in CONCURRENCY_EVENT_LINES),
                'Event-dependent required verification is unsupported')
        require(not re.search(r'\bcontinue-on-error\s*:', line),
                'Tolerated verification failures are unsupported')
        if section != 'jobs':
            continue
        match = re.fullmatch(r'  ([a-zA-Z0-9_-]+):', line)
        if match:
            current = match[1]
            require(current not in jobs, 'Duplicate workflow job')
            jobs[current] = []
        elif line != 'jobs:':
            require(current is not None and line.startswith('    '),
                    'Unsupported workflow job layout')
            jobs[current].append(line)
    expected = set(REQUIRED_JOBS) | {job + '-workloads' for job in REQUIRED_JOBS}
    require(set(jobs) == expected | {'revision'}, 'Unsupported required-gate job inventory')
    result = {}
    for job_id in expected:
        body = jobs[job_id]
        require(body.count('    steps:') == 1, 'Unsupported verification steps layout')
        split = body.index('    steps:')
        header, body = body[:split], body[split + 1:]
        header_keys = []
        for line in header:
            if line.startswith('    ') and not line.startswith('     '):
                match = re.fullmatch(r'    ([a-z-]+):(?: .*)?', line)
                require(match is not None, 'Unsupported job property layout')
                header_keys.append(match[1])
        require(len(header_keys) == len(set(header_keys)), 'Duplicate job properties')
        aggregate = job_id in REQUIRED_JOBS
        if aggregate:
            require(set(header_keys) <= {'name', 'needs', 'if', 'runs-on', 'timeout-minutes'},
                    'Unsupported aggregate job properties')
            require('    name: ' + REQUIRED_JOBS[job_id] in header
                    and '    needs: [' + job_id + '-workloads]' in header
                    and '    if: always()' in header,
                    'Unsupported aggregate gate contract')
            names = {None: REQUIRED_JOBS[job_id]}
        else:
            base = job_id.removesuffix('-workloads')
            platform, workloads = WORKLOADS[base]
            require(set(header_keys) <= {'name', 'runs-on', 'timeout-minutes', 'strategy'},
                    'Unsupported workload job properties')
            require('    name: ' + platform + ' / ${{ matrix.workload }}' in header,
                    'Unsupported workload job name')
            require('    strategy:' in header, 'Missing workload matrix')
            start = header.index('    strategy:')
            end = next((i for i in range(start + 1, len(header))
                        if header[i].startswith('    ') and not header[i].startswith('     ')),
                       len(header))
            require(header[start:end] == [
                '    strategy:', '      fail-fast: false', '      matrix:',
                '        workload: [' + ', '.join(workloads) + ']'],
                'Unsupported workload matrix')
            names = {workload: platform + ' / ' + workload for workload in workloads}
        for name in names.values():
            result[name] = []
        blocks = []
        for line in body:
            if line.startswith('      - '):
                require(re.fullmatch(r'      - name: [^\'"{}]+', line),
                        'Unsupported verification step name/layout')
                blocks.append([line])
            else:
                require(blocks and line.startswith('        '), 'Unsupported step properties')
                blocks[-1].append(line)
        for block in blocks:
            step_name = block[0].removeprefix('      - name: ')
            properties = {}
            for line in block[1:]:
                if line.startswith('        ') and not line.startswith('         '):
                    match = re.fullmatch(r'        ([a-z-]+):(?: (.*))?', line)
                    require(match and match[1] not in properties, 'Unsupported step property layout')
                    properties[match[1]] = match[2] or ''
            require(set(properties) <= {'id', 'if', 'run', 'uses', 'env', 'with', 'shell',
                                         'working-directory', 'timeout-minutes'},
                    'Unsupported step properties')
            require(('run' in properties) != ('uses' in properties), 'Unsupported step execution')
            if 'run' not in properties:
                continue
            condition = properties.get('if')
            selected = names
            if condition is not None:
                match = re.fullmatch(r"matrix\.workload == '([a-z-]+)'", condition)
                require(not aggregate and match and match[1] in names,
                        'Unsupported conditional verification step')
                selected = {match[1]: names[match[1]]}
            for name in selected.values():
                result[name].append(step_name)
        require(all(result[name] for name in names.values()), 'Missing workload verification steps')
    require(all(len(names) == len(set(names)) for names in result.values()),
            'Ambiguous verification step names')
    return result

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
