#!/usr/bin/env python3
"""Summarize one completed GitHub CI attempt; emit compact JSON, never raw logs."""
import argparse
from datetime import datetime
import json
import re
import subprocess

REQUIRED = ('Native checks', 'WebAssembly core check', 'macOS development app bundles')


def seconds(start, end):
    return (datetime.fromisoformat(end.replace('Z', '+00:00')) -
            datetime.fromisoformat(start.replace('Z', '+00:00'))).total_seconds()


def summarize(run, jobs, logs):
    if run['status'] != 'completed' or any(j['status'] != 'completed' for j in jobs):
        raise ValueError('Only completed attempts can be compared')
    gates = [j for j in jobs if j['name'] in REQUIRED]
    if len(gates) != len(REQUIRED) or {j['name'] for j in gates} != set(REQUIRED):
        raise ValueError('Expected exactly the three required gates')
    start = run['run_started_at']
    rows = []
    for job in jobs:
        lines = [line for line in logs.splitlines() if line.startswith(job['name'] + '\t')]
        sizes = [int(m.group(1)) for line in lines
                 if (m := re.search(r'Cache Size:.*\((\d+) B\)', line))]
        # Actions reports this size on restoration. A missing value is unknown,
        # not zero. Save sizes can be read separately from the cache API.
        cache_steps = [s for s in job['steps'] if 'cache' in s['name'].lower()
                       and s.get('conclusion') != 'skipped'
                       and s.get('started_at') and s.get('completed_at')]
        save_steps = [s for s in cache_steps if 'save' in s['name'].lower()
                      or s['name'].startswith('Post ')]
        restore_steps = [s for s in cache_steps if s not in save_steps
                         and 'identify' not in s['name'].lower()]
        rows.append({
            'name': job['name'], 'conclusion': job['conclusion'],
            'start_offset_seconds': seconds(start, job['started_at']),
            'duration_seconds': seconds(job['started_at'], job['completed_at']),
            'cache_restore_seconds': sum(seconds(s['started_at'], s['completed_at']) for s in restore_steps),
            'cache_save_seconds': sum(seconds(s['started_at'], s['completed_at']) for s in save_steps),
            'restored_archive_bytes': sizes or None,
            'restored_keys': [line.split('Cache restored from key: ', 1)[1] for line in lines
                              if 'Cache restored from key: ' in line],
            'saved_keys': [line.split('Cache saved with key: ', 1)[1] for line in lines
                           if 'Cache saved with key: ' in line],
            'cache_misses': sum('Cache not found for input keys:' in line for line in lines),
        })
    return {
        'url': run['html_url'], 'event': run['event'], 'sha': run['head_sha'],
        'attempt': run['run_attempt'], 'conclusion': run['conclusion'],
        'initial_runner_wait_seconds': min(row['start_offset_seconds'] for row in rows),
        'required_check_seconds': max(seconds(start, j['completed_at']) for j in gates),
        'runner_minutes': round(sum(row['duration_seconds'] for row in rows) / 60, 2),
        'jobs': rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run_id', type=int)
    parser.add_argument('--repo', default='titan-engine/titan')
    parser.add_argument('--attempt', type=int, help='Defaults to latest attempt')
    args = parser.parse_args()

    def gh(*arguments):
        return subprocess.check_output(['gh', *arguments], text=True)

    path = f'repos/{args.repo}/actions/runs/{args.run_id}'
    run = json.loads(gh('api', path))
    attempt = args.attempt or run['run_attempt']
    run = json.loads(gh('api', f'{path}/attempts/{attempt}'))
    pages = json.loads(gh('api', '--paginate', '--slurp',
                         f'{path}/attempts/{attempt}/jobs?per_page=100'))
    jobs = [job for page in pages for job in page['jobs']]
    logs = gh('run', 'view', str(args.run_id), '--repo', args.repo,
              '--attempt', str(attempt), '--log')
    print(json.dumps(summarize(run, jobs, logs), indent=2))


if __name__ == '__main__':
    main()
