#!/usr/bin/env python3
"""Actual native GPU host, authenticated inspector, and tick-by-tick route replay.

Requires a native graphical session. This does not silently substitute a headless
renderer; successful completion includes presented GPU frame evidence.
"""
import json
import os
from pathlib import Path
import sys
import tempfile
import time

GAME = Path(__file__).resolve().parents[1]
REPO = GAME.parents[1]
sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes


def main():
    processes.run(['cargo', 'build', '--manifest-path', str(GAME / 'Cargo.toml'),
                   '--features', 'player', '--bin', 'play'], phase='build', check=True)
    processes.run(['cargo', 'build', '-p', 'titan-cli'], cwd=REPO, phase='build', check=True)

    def target(root):
        result = processes.run(['cargo', 'metadata', '--no-deps', '--format-version', '1',
                                '--manifest-path', str(root / 'Cargo.toml')],
                               phase='build', capture_output=True, text=True, check=True)
        return Path(json.loads(result.stdout)['target_directory']) / 'debug'

    binary, cli = target(GAME) / 'play', target(REPO) / 'titan'
    instance = f'collection-player-test-{os.getpid()}'

    def call(*args, success=True):
        result = processes.run([str(cli), '--format', 'json', '--project', str(GAME),
                               '--instance', instance, *map(str, args)],
                              capture_output=True, text=True)
        data = json.loads(result.stdout)
        assert (result.returncode == 0) == success, data
        return data

    def invoke(name, arguments=None):
        return call('invoke', name, '--arguments', json.dumps(arguments or {}))

    def state():
        return call('query', 'state')['response']['value']

    def playback():
        return call('query', 'playback')['response']['value']

    with tempfile.TemporaryDirectory(prefix='collection-player-') as directory:
        log_path = Path(directory) / 'player.log'
        with log_path.open('w') as log:
            process = processes.Popen([str(binary), '--paused', '--inspect', '--allow-control',
                                       '--project', str(GAME), '--instance', instance,
                                       '--run-for-ms', '30000'], project=GAME, instance=instance,
                                      cwd=GAME, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 15
                while not call('instances')['instances']:
                    assert process.poll() is None, log_path.read_text()
                    assert time.monotonic() < deadline, 'player discovery timed out'
                    time.sleep(.05)
                assert state()['session_tick'] == 0
                # Capture stays unregistered until the separate capture integration.
                assert call('capture', success=False)['error']['code'] == 'unsupported_operation'
                route = ['right'] * 8 + ['up'] * 20 + ['right'] * 16
                for frame, action in enumerate(route, 1):
                    call('input', frame, '--actions', json.dumps({action: {'kind': 'button', 'value': True}}))
                call('step', 44)
                expected = state()
                assert expected['completed'] and expected['position'] == {'x': 3000, 'z': -2000}, expected
                recording = call('query', 'recording')['response']['value']
                invoke('load_replay', {'recording': recording})
                assert playback()['position'] == 0
                call('step', 1)
                assert state()['position'] == {'x': -2750, 'z': 3000}
                invoke('resume')
                deadline = time.monotonic() + 10
                while not playback()['complete']:
                    assert process.poll() is None, log_path.read_text()
                    assert time.monotonic() < deadline, 'interactive replay timed out'
                    time.sleep(.02)
                actual = state()
                for key in ['position', 'collected', 'completed', 'remaining', 'session_tick']:
                    assert actual[key] == expected[key], (key, actual, expected)
                assert playback()['paused']
                call('step', 1, success=False)
                invoke('restart')
                assert state()['session_tick'] == 0 and playback()['position'] == 0
                time.sleep(.15)
            finally:
                processes.terminate(process)
        output = log_path.read_text()
        print(output)
        assert 'GPU frames;' in output, 'native player did not finish GPU presentation normally'
        rendered = int(output.split('rendered ')[-1].split(' GPU frames;')[0])
        assert rendered > 0, output
        print(json.dumps({'native_gpu_frames': rendered, 'replay_ticks': 44,
                          'semantic_equivalence': True, 'capture_registered': False}))


if __name__ == '__main__':
    with processes.harness_deadline():
        main()
