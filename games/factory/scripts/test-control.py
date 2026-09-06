#!/usr/bin/env python3
"""Bounded factory acceptance through native discovery, CLI and sequence runner."""
import json
import os
from pathlib import Path
import sys
import time

GAME = Path(__file__).resolve().parents[1]
REPO = GAME.parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes
from acceptance_evidence import FailureEvidence


def main(failures, log):
    def run(args, **kwargs):
        failures.record_command(args, None)
        result = processes.run(args, **kwargs)
        failures.record_command(args, result)
        result.check_returncode()
        return result

    for manifest, extra in [(GAME / 'Cargo.toml', ['--bin', 'titan-factory']),
                            (REPO / 'Cargo.toml', ['-p', 'titan-cli'])]:
        run(['cargo', 'build', '--manifest-path', str(manifest), *extra],
            phase='build', stdout=log, stderr=log)

    def target(root):
        result = run(['cargo', 'metadata', '--no-deps', '--format-version', '1',
                      '--manifest-path', str(root / 'Cargo.toml')],
                     phase='build', capture_output=True, text=True)
        return Path(json.loads(result.stdout)['target_directory']) / 'debug'

    binary, cli = target(GAME) / 'titan-factory', target(REPO) / 'titan'
    instance = f'factory-test-{os.getpid()}'

    def call(*args, error=None):
        command = [str(cli), '--format', 'json', '--project', str(GAME),
                   '--instance', instance, *map(str, args)]
        failures.record_command(command, None)
        result = processes.run(command, capture_output=True, text=True)
        failures.record_command(command, result)
        data = json.loads(result.stdout)
        failures.observe(data)
        if error:
            assert result.returncode != 0 and data['error']['code'] == error, data
        else:
            assert result.returncode == 0 and data['status'] == 'success', data
        return data

    def state():
        return call('query', 'state')['response']['value']

    def invoke(name, arguments=None, **kwargs):
        return call('invoke', name, '--arguments', json.dumps(arguments or {}), **kwargs)

    process = processes.Popen([str(binary), '--serve', '--instance', instance,
                               '--run-for-ms', '120000', '--allow-mutation'],
                              project=GAME, instance=instance, cwd=GAME, stdout=log, stderr=log)
    failures.record_process(process)
    try:
        deadline = time.monotonic() + 10
        while not call('instances')['instances']:
            assert process.poll() is None, 'runtime exited before discovery'
            assert time.monotonic() < deadline, 'discovery exceeded 10 seconds'
            time.sleep(.05)
        initial = state()
        assert len(initial['structures']) == 1
        # UI and protocol consume one immutable explanation model. Repeated
        # reads and prospective edits must not record commands or tick the world.
        before_status = call('status')['response']
        before_recording = call('query', 'recording')['response']['value']
        for _ in range(3):
            interface = call('query', 'interface')['response']['value']
            assert interface['structures'] == initial['structures']
            preview = call('query', 'preview', '--arguments', json.dumps(
                {'x': 2, 'y': 3, 'action': 'place'}))['response']['value']
            assert preview['valid'] is True, preview
            assert state() == initial
        assert call('status')['response'] == before_status
        assert call('query', 'recording')['response']['value'] == before_recording
        initial_capture = call('capture')['response']
        operations = json.loads((GAME / 'tests/construction.json').read_text())
        invoke('sequence', {'operations': operations})
        built = state()
        assert built['tick'] == 60 and built['delivered'] == 0
        assert [(s['x'], s['y'], s['kind'], s['facing']) for s in built['structures']] == [
            (1, 3, 'extractor', 'E'), (2, 3, 'conveyor', 'S'), (10, 3, 'delivery', 'E')]
        assert call('capture')['response']['checksum'] != initial_capture['checksum']
        for operation in [dict(op='remove', x=10, y=3), dict(op='rotate', x=0, y=0),
                          dict(op='place', kind='conveyor', x=-1, y=0, facing='E')]:
            before = state()
            result = invoke('construct', operation, error='invalid_value')
            assert result['error']['message']
            assert state() == before
        # Separate standalone sequence execution must agree with the controlled simulation.
        result = run([str(binary), '--sequence', str(GAME / 'tests/construction.json')],
                     cwd=GAME, capture_output=True, text=True)
        replay = json.loads(result.stdout)
        assert replay['state']['structures'] == built['structures']
        assert replay['state']['tick'] == built['tick']
        assert sum('error' in outcome for outcome in replay['outcomes']) == 5
        frame = call('status')['response']['current_frame']
        invoke('restart')
        assert call('status')['response']['current_frame'] == frame
        reset = state()
        assert reset['tick'] == 0
        assert reset['structures'] == initial['structures']
        assert reset['camera'] == initial['camera']
        invoke('sequence', {'operations': operations})
        assert state()['structures'] == built['structures']
        print('Factory native commands, invalid operations, capture, deterministic sequence and restart passed.')
    finally:
        processes.terminate(process)


if __name__ == '__main__':
    with FailureEvidence('factory-control', repo=REPO) as failures:
        with failures.runtime_log() as log:
            main(failures, log)
