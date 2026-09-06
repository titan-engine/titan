#!/usr/bin/env python3
"""Build and measure the deterministic swarm in fresh processes (macOS/Linux)."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import acceptance_process as processes
import sys
from measurement_process import measure

REPO = Path(__file__).resolve().parents[1]


def integer(minimum):
    def parse(value):
        number = int(value)
        if number < minimum:
            raise argparse.ArgumentTypeError(f'must be at least {minimum}')
        return number
    return parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--counts', nargs='+', type=integer(0), default=[1000, 10000])
    parser.add_argument('--steps', type=integer(0), default=120)
    parser.add_argument('--repeats', type=integer(1), default=3)
    parser.add_argument('--timeout-seconds', type=integer(1), default=120)
    parser.add_argument('--threads', type=integer(1), default=1, help='1 preserves sequential execution; larger values opt into bounded parallel execution')
    parser.add_argument('--debug', action='store_true', help='use dev build for smoke tests')
    args = parser.parse_args()
    if sys.platform not in ('darwin', 'linux'):
        parser.error('process RSS measurement supports macOS and Linux; run the Rust example directly elsewhere')
    build = ['cargo', 'build', '--locked', '-p', 'titan', '--example', 'swarm', '--message-format=json']
    if not args.debug:
        build.append('--release')
    result = processes.run(build, phase="build", cwd=REPO, check=True, stdout=subprocess.PIPE, text=True)
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    executable = next(item['executable'] for item in artifacts
                      if item.get('reason') == 'compiler-artifact'
                      and item['target']['name'] == 'swarm' and item.get('executable'))
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
        'memory_scope': 'whole child process high-water RSS, including setup, simulation and correctness checks; excludes Cargo and this runner',
        'wall_scope': 'whole child lifetime including launch and checks; polling resolution approximately 10ms',
        'samples': [],
    }
    for count in args.counts:
        for repeat in range(args.repeats):
            sample = measure([executable, '--entities', str(count), '--steps', str(args.steps), '--threads', str(args.threads)],
                             args.timeout_seconds)
            sample.update(entities=count, repeat=repeat + 1)
            report['samples'].append(sample)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
