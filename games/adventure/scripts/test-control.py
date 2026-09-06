#!/usr/bin/env python3
"""Bounded native acceptance through the real discovery server and CLI."""
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

    for manifest, extra in [(GAME / 'Cargo.toml', ['--bin', 'titan-adventure']),
                            (REPO / 'Cargo.toml', ['-p', 'titan-cli'])]:
        run(['cargo', 'build', '--manifest-path', str(manifest), *extra],
            phase='build', stdout=log, stderr=log)

    def target(root):
        result = run(['cargo', 'metadata', '--no-deps', '--format-version', '1',
                      '--manifest-path', str(root / 'Cargo.toml')],
                     phase='build', capture_output=True, text=True)
        return Path(json.loads(result.stdout)['target_directory']) / 'debug'

    binary, cli = target(GAME) / 'titan-adventure', target(REPO) / 'titan'
    instance = f'adventure-test-{os.getpid()}'

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

    def drive(actions):
        frame = call('status')['response']['current_frame']
        for offset, action in enumerate(actions, 1):
            call('input', frame + offset, '--actions', json.dumps({action: {'kind': 'button', 'value': True}}))
        call('step', len(actions))

    def start(mutation):
        process = processes.Popen([str(binary), '--serve', '--instance', instance,
                                   '--run-for-ms', '120000', *(['--allow-mutation'] if mutation else [])],
                                  project=GAME, instance=instance, cwd=GAME, stdout=log, stderr=log)
        failures.record_process(process)
        try:
            deadline = time.monotonic() + 10
            while not any(item['instance_id'] == instance for item in call('instances')['instances']):
                assert process.poll() is None, 'runtime exited before discovery'
                assert time.monotonic() < deadline, 'discovery exceeded 10 seconds'
                time.sleep(.05)
            return process
        except BaseException:
            processes.terminate(process)
            raise

    process = start(True)
    trace = []
    try:
        initial = state()
        entities = call('entities')['response']['entities']
        names = [entity['name'] for entity in entities]
        assert len(names) == len(set(names)), names
        for character in ('jumper', 'strong'):
            entity = next(entity for entity in entities if entity['name'] == character)
            detail = call('entity', entity['id']['index'], entity['id']['generation'])['response']
            key = next(key for key in detail['components'] if key.endswith('::Position'))
            assert all(detail['components'][key][axis] == initial['characters'][character][axis] for axis in ('x', 'y', 'z'))
            assert all(not detail['component_fields'][key][axis]['writable'] for axis in ('x', 'z'))
        route = json.loads((GAME / 'tests/control-route.json').read_text())
        for sample in route:
            frame = call('status')['response']['current_frame'] + 1
            call('input', frame, '--actions', json.dumps({a: {'kind': 'button', 'value': True} for a in sample['actions']}))
            call('step', 1)
            current = state()
            assert current['active_character'] == sample['active_character'], (current, sample)
            for name, position in sample['characters'].items():
                assert all(current['characters'][name][axis] == value for axis, value in position.items()), (current, sample)
                assert current['characters'][name]['y'] == 0, current
            trace.append(current)
        recording = call('query', 'recording')['response']['value']
        expected = state()
        invoke('replay', {'recording': recording})
        for key in ('characters', 'active_character', 'session_tick', 'consumed_input'):
            assert state()[key] == expected[key], (key, state(), expected)
        before = state()
        invoke('replay', {'recording': {**recording, 'fixture': 'wrong'}}, error='invalid_value')
        assert state() == before
        frame = call('status')['response']['current_frame']
        call('input', frame + 1, '--actions', '{"right":{"kind":"button","value":true}}')
        invoke('restart')
        assert call('status')['response']['current_frame'] == frame
        call('step', 1)
        assert state()['characters'] == initial['characters']
        assert state()['active_character'] == 'jumper'
        invoke('switch')
        assert state()['active_character'] == 'strong'
        # Inject one complete snapshot at a time: reconstruction must not
        # manufacture edges when the next host frame continues the same hold.
        def sample(actions):
            frame = call('status')['response']['current_frame'] + 1
            call('input', frame, '--actions', json.dumps({a: {'kind': 'button', 'value': True} for a in actions}))
            call('step', 1)
            return state()
        for held in ('restart', 'jump', 'switch'):
            invoke('restart')
            sample([])
            first = sample(list(dict.fromkeys(['restart', held])))
            continuing = sample([held])
            assert continuing['session_generation'] == first['session_generation'], (held, continuing)
            assert continuing['active_character'] == 'jumper' and continuing['characters']['jumper']['y'] == 0, (held, continuing)
            sample([])
            fresh = sample([held])
            if held == 'restart':
                assert fresh['session_generation'] == first['session_generation'] + 1
            elif held == 'jump':
                assert fresh['characters']['jumper']['y'] == 170
            else:
                assert fresh['active_character'] == 'strong'
            recording = call('query', 'recording')['response']['value']
            expected_after_boundary = state()
            invoke('replay', {'recording': recording})
            for key in ('characters', 'active_character', 'consumed_input', 'session_tick'):
                assert state()[key] == expected_after_boundary[key], (held, key)
        call('capture', error='unsupported')
    finally:
        try:
            processes.graceful_shutdown(process)
        finally:
            processes.terminate(process)
    assert not any(item['instance_id'] == instance for item in call('instances')['instances']), 'shutdown left owned discovery registration'
    if '--trace' in sys.argv:
        Path(sys.argv[sys.argv.index('--trace') + 1]).write_text(json.dumps(trace))
    print('Adventure native CLI: named fields, per-tick control route, held switching, replay, restart, switch command and cleanup passed.')


if __name__ == '__main__':
    with FailureEvidence('adventure-control', repo=REPO) as failures:
        with failures.runtime_log() as log:
            main(failures, log)
