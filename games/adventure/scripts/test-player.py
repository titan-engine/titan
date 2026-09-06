#!/usr/bin/env python3
"""Actual native GPU host, authenticated inspector, and tick-by-tick route replay.

Requires a native graphical session. This does not silently substitute a headless
renderer; successful completion includes presented GPU frame evidence.
"""
import base64
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
from acceptance_evidence import FailureEvidence, sanitize, _png, _read_regular, IMAGE_LIMIT


def main(failures):
    processes.run(['cargo', 'build', '--manifest-path', str(GAME / 'Cargo.toml'),
                   '--features', 'player', '--bin', 'play'], phase='build', check=True)
    processes.run(['cargo', 'build', '-p', 'titan-cli'], cwd=REPO, phase='build', check=True)

    def target(root):
        result = processes.run(['cargo', 'metadata', '--no-deps', '--format-version', '1',
                                '--manifest-path', str(root / 'Cargo.toml')],
                               phase='build', capture_output=True, text=True, check=True)
        return Path(json.loads(result.stdout)['target_directory']) / 'debug'

    binary, cli = target(GAME) / 'play', target(REPO) / 'titan'
    instance = f'adventure-player-test-{os.getpid()}'

    def call(*args, success=True):
        result = processes.run([str(cli), '--format', 'json', '--project', str(GAME),
                               '--instance', instance, *map(str, args)],
                              capture_output=True, text=True)
        failures.record_command(args, result)
        data = json.loads(result.stdout)
        if data.get('error'):
            failures.observe(data)
        assert (result.returncode == 0) == success, data
        return data

    def invoke(name, arguments=None):
        return call('invoke', name, '--arguments', json.dumps(arguments or {}))

    def state():
        return call('query', 'state')['response']['value']

    def playback():
        return call('query', 'playback')['response']['value']

    root = REPO / 'target' / 'adventure-gpu-evidence'
    root.mkdir(parents=True, exist_ok=True)
    directory = tempfile.mkdtemp(prefix='native-', dir=root)
    captures = {}

    def capture(name):
        before = call('status')
        outcome = call('capture')
        after = call('status')
        value = outcome['response']
        identity = value['identity']
        for key in ('observed_frame', 'state_revision'):
            assert before[key] == outcome[key] == identity[key] == after[key], (key, before[key], identity, after[key])
        assert identity['instance_id'] == instance and identity['capture_id'] > 0
        assert (identity['width'], identity['height']) == (960, 540)
        assert value['format'] == 'png' and (value['width'], value['height']) == (960, 540), value
        artifact = value['artifact']
        if artifact.startswith('data:image/png;base64,'):
            raw = base64.b64decode(artifact.split(',', 1)[1], validate=True)
        else:
            raw = _read_regular(Path(artifact), IMAGE_LIMIT)
        assert len(raw) <= IMAGE_LIMIT
        raw = _png(raw)
        failures.files['capture.png'] = raw
        destination = Path(directory) / f'{name}.png'
        destination.write_bytes(raw)
        destination.chmod(0o600)
        captures[name] = {**value, 'artifact': destination.name, 'state': state()}
        failures.checkpoint('capture')
        return value

    try:
        log_path = Path(directory) / 'player.log'
        with log_path.open('w') as log:
            process = processes.Popen([str(binary), '--paused', '--verify-surface-lifecycle', '--inspect', '--allow-control',
                                       '--project', str(GAME), '--instance', instance,
                                       '--run-for-ms', '30000'], project=GAME, instance=instance,
                                      cwd=GAME, stdout=log, stderr=log)
            failures.record_process(process)
            try:
                deadline = time.monotonic() + 15
                while not call('instances')['instances']:
                    assert process.poll() is None, log_path.read_text()
                    assert time.monotonic() < deadline, 'player discovery timed out'
                    time.sleep(.05)
                while 'surface lifecycle verified:' not in log_path.read_text():
                    assert process.poll() is None, 'native host exited during lifecycle verification'
                    assert time.monotonic() < deadline, 'OS resize presentation timed out'
                    time.sleep(.05)
                # Startup focus/resize callbacks are external revisions; settle them
                # before testing capture's own no-tick/no-revision guarantee.
                time.sleep(.5)
                assert state()['session_tick'] == 0
                initial_capture = capture('initial')
                assert capture('initial-repeat')['checksum'] == initial_capture['checksum']
                route = json.loads((GAME / 'tests/control-route.json').read_text())
                for frame, sample in enumerate(route, 1):
                    call('input', frame, '--actions', json.dumps({action: {'kind': 'button', 'value': True} for action in sample['actions']}))
                call('step', len(route))
                expected = state()
                assert expected['characters'] == route[-1]['characters'] and expected['active_character'] == route[-1]['active_character'], expected
                win_capture = capture('moved')
                assert win_capture['checksum'] != initial_capture['checksum']
                recording = call('query', 'recording')['response']['value']
                invoke('load_replay', {'recording': recording})
                assert playback()['position'] == 0
                call('step', 1)
                assert state()['characters']['jumper'] == {'x': 1560, 'z': 6500}
                invoke('resume')
                deadline = time.monotonic() + 10
                while not playback()['complete']:
                    assert process.poll() is None, log_path.read_text()
                    assert time.monotonic() < deadline, 'interactive replay timed out'
                    # Native focus loss pauses by design; another acceptance
                    # browser may take focus while this explicit replay runs.
                    if playback()['paused']:
                        invoke('resume')
                    time.sleep(.02)
                actual = state()
                for key in ['characters', 'active_character', 'session_tick', 'consumed_input']:
                    assert actual[key] == expected[key], (key, actual, expected)
                assert playback()['paused']
                assert capture('replay')['checksum'] == win_capture['checksum']
                call('step', 1, success=False)
                invoke('restart')
                assert state()['session_tick'] == 0 and playback()['position'] == 0
                reset_capture = capture('reset')
                assert reset_capture['checksum'] == initial_capture['checksum']
                assert reset_capture['identity']['session_generation'] > win_capture['identity']['session_generation']
                invoke('switch')
                assert state()['active_character'] == 'strong'
                assert capture('switched')['checksum'] != initial_capture['checksum']
                time.sleep(.15)
            finally:
                processes.terminate(process)
                log.flush()
                print(log_path.read_text(), flush=True)
        output = log_path.read_text()
        assert 'surface lifecycle verified:' in output, output
        assert 'native GPU adapter:' in output, output
        assert 'GPU frames;' in output, 'native player did not finish GPU presentation normally'
        rendered = int(output.split('rendered ')[-1].split(' GPU frames;')[0])
        assert rendered > 0, output
        # A second actual GPU host verifies that capture needs no mutation opt-in.
        with log_path.open('a') as log:
            process = processes.Popen([str(binary), '--paused', '--inspect',
                                       '--project', str(GAME), '--instance', instance,
                                       '--run-for-ms', '15000'], project=GAME, instance=instance,
                                      cwd=GAME, stdout=log, stderr=log)
            failures.record_process(process)
            try:
                deadline = time.monotonic() + 10
                while not call('instances')['instances']:
                    assert process.poll() is None, 'read-only GPU host exited'
                    assert time.monotonic() < deadline, 'read-only discovery timed out'
                    time.sleep(.05)
                time.sleep(.5)
                assert not call('capabilities')['response']['mutation_enabled']
                rejected = call('invoke', 'switch', '--arguments', '{}', success=False)
                assert rejected['error']['code'] == 'mutation_disabled'
                assert capture('readonly')['checksum'] == initial_capture['checksum']
            finally:
                processes.terminate(process)
        summary = {'native_gpu_frames': rendered, 'replay_ticks': len(route),
                          'semantic_equivalence': True, 'final_state': actual, 'capture_registered': True, 'readonly_capture': True, 'surface_lifecycle_verified': True, 'captures': captures}
        (Path(directory) / 'evidence.json').write_text(json.dumps(sanitize(summary), indent=2))
        print(json.dumps({**summary, 'evidence_directory': directory}))
    finally:
        # Retain only bounded sanitized text and known PNGs, never discovery data.
        if log_path.exists():
            log_path.write_text(sanitize(log_path.read_text()[-128 * 1024:]))
            log_path.chmod(0o600)


if __name__ == '__main__':
    with FailureEvidence('adventure-player', repo=REPO) as failures:
        main(failures)
