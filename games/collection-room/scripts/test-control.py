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

    for manifest, extra in [(GAME / 'Cargo.toml', ['--bin', 'titan-collection-room']),
                            (REPO / 'Cargo.toml', ['-p', 'titan-cli'])]:
        run(['cargo', 'build', '--locked', '--manifest-path', str(manifest), *extra],
            phase='build', stdout=log, stderr=log)

    def target(root):
        result = run(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1',
                      '--manifest-path', str(root / 'Cargo.toml')],
                     phase='build', capture_output=True, text=True)
        return Path(json.loads(result.stdout)['target_directory']) / 'debug'

    binary, cli = target(GAME) / 'titan-collection-room', target(REPO) / 'titan'
    instance = f'collection-room-test-{os.getpid()}'

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
            while not call('instances')['instances']:
                assert process.poll() is None, 'runtime exited before discovery'
                assert time.monotonic() < deadline, 'discovery exceeded 10 seconds'
                time.sleep(.05)
            return process
        except BaseException:
            processes.terminate(process)
            raise

    process = start(True)
    try:
        assert call('capabilities')['response']['mutation_enabled']
        entities = call('entities')['response']['entities']
        names = [entity['name'] for entity in entities]
        assert len(names) == len(set(names)) and 'player' in names, names
        player = next(entity['id'] for entity in entities if entity['name'] == 'player')
        detail = call('entity', player['index'], player['generation'])['response']
        assert {'floor', 'obstacle-1', 'obstacle-2', 'collectible-1', 'collectible-2', 'collectible-3'}.issubset(names), names
        position_key = next(key for key in detail['components'] if key.endswith('::Position'))
        assert detail['components'][position_key] == {'x': -3000, 'z': 3000}
        assert all(not detail['component_fields'][position_key][axis]['writable'] for axis in ('x', 'z'))
        progress_key = next(key for key in detail['components'] if key.endswith('::Progress'))
        assert detail['components'][progress_key]['collected'] == 0
        initial = state()
        assert initial['position'] == {'x': -3000, 'z': 3000} and initial['collected'] == 0
        assert initial['total'] == 3 and not initial['completed']
        for position in [{'x': 5000, 'z': 0}, {'x': 0, 'z': 0}, {'x': 'bad', 'z': 0}]:
            before = call('status')
            rejected = invoke('teleport', position, error='invalid_value')
            assert (rejected['observed_frame'], rejected['state_revision']) == (before['observed_frame'], before['state_revision'])
            assert state() == initial, 'rejected teleport changed game state'
        # Inspect only the bounded, sanitized snapshot supplied by FailureEvidence.
        bundle = json.loads(failures.files['bundle.json'])
        assert bundle['world_state']['game']['position'] == initial['position']
        assert 'capture.png' not in failures.files
        failures.checkpoint('diagnostic')

        invoke('teleport', {'x': -3000, 'z': 0})
        drive(['right'] * 20)
        assert state()['position']['x'] < -750, 'player crossed central obstacle'
        invoke('restart')
        route = ['right'] * 8 + ['up'] * 20 + ['right'] * 16
        drive(route)
        won = state()
        assert won['position'] == {'x': 3000, 'z': -2000}, won
        assert won['collected'] == won['total'] == 3 and won['completed'], won
        assert won['remaining'] == []
        recording = call('query', 'recording')['response']['value']
        assert len(recording['frames']) == 44 and not recording['truncated'], recording
        call('step', 4)
        assert state()['collected'] == 3, 'collected an item twice'
        invoke('replay', {'recording': recording})
        replayed = state()
        for key in ['position', 'collected', 'total', 'completed', 'remaining', 'session_tick']:
            assert replayed[key] == won[key], (key, replayed, won)
        # Restart preserves the host clock while clearing future inputs and progress.
        frame = call('status')['response']['current_frame']
        call('input', frame + 1, '--actions', '{"right":{"kind":"button","value":true}}')
        invoke('restart')
        assert call('status')['response']['current_frame'] == frame
        call('step', 1)
        restarted = state()
        assert restarted['position'] == initial['position'] and restarted['collected'] == 0
        assert not restarted['completed'] and restarted['remaining'] == initial['remaining']
    finally:
        try:
            processes.graceful_shutdown(process)
        finally:
            processes.terminate(process)
    assert not call('instances')['instances'], 'shutdown left discovery registration'

    process = start(False)
    try:
        assert not call('capabilities')['response']['mutation_enabled']
        before = state()
        invoke('teleport', {'x': -2000, 'z': 3000}, error='mutation_disabled')
        assert state() == before
    finally:
        try:
            processes.graceful_shutdown(process)
        finally:
            processes.terminate(process)
    assert not call('instances')['instances']
    print('Collection room native CLI: discovery, named fields, policy, transactional teleport, blocked movement, 44-tick win/replay, restart and bounded diagnostics passed.')


if __name__ == '__main__':
    with FailureEvidence('collection-room-control', repo=REPO) as failures:
        with failures.runtime_log() as log:
            main(failures, log)
