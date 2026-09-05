#!/usr/bin/env python3
"""Measure deterministic mixed ECS schedules in fresh processes (macOS/Linux)."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time

import acceptance_process as processes


REPO = Path(__file__).resolve().parents[1]


def integer(minimum):
    def parse(value):
        number = int(value)
        if number < minimum:
            raise argparse.ArgumentTypeError(f'must be at least {minimum}')
        return number
    return parse


def measure(command, timeout):
    with tempfile.TemporaryFile(mode='w+') as output:
        started = time.monotonic()
        process = subprocess.Popen(command, cwd=REPO, stdout=output, start_new_session=True)
        try:
            while True:
                pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                if time.monotonic() - started >= timeout:
                    raise TimeoutError(f'mixed schedule exceeded {timeout}s: {command}')
                time.sleep(0.01)
        finally:
            processes.terminate(process)
        elapsed = time.monotonic() - started
        if process.returncode:
            raise RuntimeError(f'mixed schedule exited {process.returncode}: {command}')
        output.seek(0)
        workload = json.load(output)
    return {
        'process_wall_seconds': elapsed,
        'peak_process_rss_bytes': usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024),
        'workload': workload,
    }


def cpu_model():
    if sys.platform == 'darwin':
        return processes.check_output(
            ['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip()
    if sys.platform.startswith('linux'):
        try:
            for line in Path('/proc/cpuinfo').read_text().splitlines():
                if line.startswith(('model name', 'Hardware')):
                    return line.split(':', 1)[1].strip()
        except (OSError, IndexError):
            pass
    return platform.processor() or 'unavailable'


def hoist_schedule_shapes(report, sample, threads):
    """Store invariant shapes once per thread limit and keep raw samples compact."""
    shapes = report['schedule_shapes_by_thread_limit'].setdefault(str(threads), {})
    for run in sample['workload']['runs']:
        for scenario in run:
            shape = scenario.pop('schedule')
            previous = shapes.setdefault(scenario['name'], shape)
            if previous != shape:
                raise RuntimeError(
                    f'schedule shape changed for {scenario["name"]} at thread limit {threads}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--counts', nargs='+', type=integer(0), default=[0, 64, 1000, 10000])
    parser.add_argument('--steps', type=integer(0), default=120)
    parser.add_argument('--work-iterations', type=integer(1), default=16)
    parser.add_argument('--repeats', type=integer(1), default=3)
    parser.add_argument('--timeout-seconds', type=integer(1), default=120)
    parser.add_argument('--threads', nargs='+', type=integer(1), default=[1, 2, 4, 8],
                        help='compare the sequential default (1) with bounded parallel limits')
    parser.add_argument('--environment-notes', default='',
                        help='record machine load, power mode, and concurrent activity notes')
    parser.add_argument('--debug', action='store_true', help='use dev build for smoke tests')
    args = parser.parse_args()
    if sys.platform not in ('darwin', 'linux'):
        parser.error('process RSS measurement supports macOS and Linux; run the Rust example directly elsewhere')
    if len(set(args.threads)) != len(args.threads):
        parser.error('--threads values must be unique')

    build = ['cargo', 'build', '-p', 'titan', '--example', 'mixed_schedule', '--message-format=json']
    if not args.debug:
        build.append('--release')
    result = processes.run(build, phase='build', cwd=REPO, check=True,
                           stdout=subprocess.PIPE, text=True)
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    executable = next(item['executable'] for item in artifacts
                      if item.get('reason') == 'compiler-artifact'
                      and item['target']['name'] == 'mixed_schedule' and item.get('executable'))
    report = {
        'schema_version': 1,
        'measured_at_utc': datetime.now(timezone.utc).isoformat(),
        'revision': processes.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
        'working_tree_dirty': bool(processes.check_output(['git', 'status', '--porcelain'], cwd=REPO)),
        'platform': platform.platform(),
        'machine': platform.machine(),
        'cpu_model': cpu_model(),
        'logical_cpus': os.cpu_count(),
        'load_average_before_samples': list(os.getloadavg()),
        'environment_notes': args.environment_notes,
        'rustc': processes.check_output(['rustc', '-vV'], text=True).strip(),
        'profile': 'dev' if args.debug else 'release',
        'rustflags': os.environ.get('RUSTFLAGS', ''),
        'memory_scope': 'whole child process high-water RSS, including all scenarios, repeated worlds and validation; excludes Cargo and this runner',
        'wall_scope': 'whole child lifetime including launch and checks; polling resolution approximately 10ms',
        'schedule_shapes_by_thread_limit': {},
        'samples': [],
    }
    sample_sequence = 0
    for repeat in range(args.repeats):
        offset = repeat % len(args.threads)
        ordered_threads = args.threads[offset:] + args.threads[:offset]
        if repeat % 2:
            ordered_threads.reverse()
        for count in args.counts:
            for threads in ordered_threads:
                sample_sequence += 1
                command = [executable, '--entities', str(count), '--steps', str(args.steps),
                           '--work-iterations', str(args.work_iterations), '--threads', str(threads)]
                sample = measure(command, args.timeout_seconds)
                hoist_schedule_shapes(report, sample, threads)
                sample.update(entities=count, max_threads=threads, repeat=repeat + 1,
                              sample_sequence=sample_sequence,
                              load_average_before=list(os.getloadavg()))
                report['samples'].append(sample)
    report['load_average_after_samples'] = list(os.getloadavg())
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
