#!/usr/bin/env python3
"""Measure sparse component retention in bounded fresh processes (macOS/Linux)."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

import acceptance_process as processes
from measurement_process import measure

REPO = Path(__file__).resolve().parents[1]
DISTRIBUTIONS = ('dense', 'rare-low', 'rare-high')


def bounded_integer(minimum, maximum):
    def parse(value):
        number = int(value)
        if not minimum <= number <= maximum:
            raise argparse.ArgumentTypeError(f'must be in {minimum}..{maximum}')
        return number
    return parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--counts', nargs='+', type=bounded_integer(1, 1_000_000), default=[1000, 100000])
    parser.add_argument('--cycles', type=bounded_integer(1, 100), default=10)
    parser.add_argument('--repeats', type=bounded_integer(1, 20), default=3)
    parser.add_argument('--distributions', nargs='+', choices=DISTRIBUTIONS, default=list(DISTRIBUTIONS))
    parser.add_argument('--timeout-seconds', type=bounded_integer(1, 600), default=120)
    parser.add_argument('--debug', action='store_true', help='use dev build for CI smoke tests')
    args = parser.parse_args()
    if any(count * args.cycles > 10_000_000 for count in args.counts):
        parser.error('entities times cycles must not exceed 10000000')
    if len(args.counts) * len(args.distributions) * args.repeats > 180:
        parser.error('at most 180 fresh processes per invocation')
    if sys.platform not in ('darwin', 'linux'):
        parser.error('RSS measurement supports macOS and Linux; run the Rust example directly elsewhere')
    build = ['cargo', 'build', '-p', 'titan', '--example', 'sparse_churn', '--message-format=json']
    if not args.debug:
        build.append('--release')
    result = processes.run(build, phase='build', cwd=REPO, check=True, stdout=subprocess.PIPE, text=True)
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    executable = next(item['executable'] for item in artifacts
                      if item.get('reason') == 'compiler-artifact'
                      and item['target']['name'] == 'sparse_churn' and item.get('executable'))
    report = {
        'schema_version': 1,
        'measured_at_utc': datetime.now(timezone.utc).isoformat(),
        'revision': processes.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
        'working_tree_dirty': bool(processes.check_output(['git', 'status', '--porcelain'], cwd=REPO)),
        'platform': platform.platform(),
        'machine': platform.machine(),
        'logical_cpus': os.cpu_count(),
        'rustc': processes.check_output(['rustc', '-vV'], text=True).strip(),
        'profile': 'dev' if args.debug else 'release',
        'rustflags': os.environ.get('RUSTFLAGS', ''),
        'parameters': vars(args),
        'memory_scope': 'whole child process peak RSS including setup, validation, snapshots and teardown; excludes Cargo/Python; not per-phase RSS or allocator live bytes',
        'wall_scope': 'whole child lifetime including launch and validation; polling resolution approximately 10ms',
        'samples': [],
    }
    for count in args.counts:
        for distribution in args.distributions:
            checksum = None
            for repeat in range(args.repeats):
                sample = measure([executable, '--entities', str(count), '--cycles', str(args.cycles),
                                  '--distribution', distribution], args.timeout_seconds)
                workload = sample['workload']
                current = workload['correctness']['checksum']
                if checksum is not None and current != checksum:
                    raise RuntimeError(f'repeat disagreement for {count} / {distribution}')
                checksum = current
                sample.update(entities=count, distribution=distribution, repeat=repeat + 1)
                report['samples'].append(sample)
    report['repeat_agreement'] = True
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
