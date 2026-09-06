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
    processes.run(['cargo', 'build', '--locked', '--manifest-path', str(GAME / 'Cargo.toml'),
                   '--features', 'player', '--bin', 'play'], phase='build', check=True)
    processes.run(['cargo', 'build', '--locked', '-p', 'titan-cli'], cwd=REPO, phase='build', check=True)

    def target(root):
        result = processes.run(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1',
                                '--manifest-path', str(root / 'Cargo.toml')],
                               phase='build', capture_output=True, text=True, check=True)
        return Path(json.loads(result.stdout)['target_directory']) / 'debug'

    binary, cli = target(GAME) / 'play', target(REPO) / 'titan'
    instance = f'collection-player-test-{os.getpid()}'

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

    root = REPO / 'target' / 'collection-room-gpu-evidence'
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

    log_path = Path(directory) / 'player.log'
    startup_log_path = Path(directory) / 'startup.log'
    try:
        # An ordinary read-only launch must tick on its own. Never repair this
        # probe with resume: a background/unfocused desktop must fail clearly.
        with startup_log_path.open('w') as log:
            process = processes.Popen([str(binary), '--trace-focus', '--inspect',
                                       '--project', str(GAME), '--instance', instance,
                                       '--run-for-ms', '20000'], project=GAME, instance=instance,
                                      cwd=GAME, stdout=log, stderr=log)
            failures.record_process(process)
            try:
                deadline = time.monotonic() + 15
                while not call('instances')['instances']:
                    assert process.poll() is None, startup_log_path.read_text()
                    assert time.monotonic() < deadline, 'ordinary player discovery timed out'
                    time.sleep(.05)
                assert not call('capabilities')['response']['mutation_enabled']
                initial_tick = state()['session_tick']
                startup_state = state()
                while startup_state['session_tick'] <= initial_tick:
                    assert process.poll() is None, startup_log_path.read_text()
                    assert time.monotonic() < deadline, (
                        'ordinary player did not start automatically; requires a focused desktop window. '
                        + startup_log_path.read_text())
                    time.sleep(.05)
                    startup_state = state()
                assert not playback()['paused'], 'ordinary player lost focus during startup probe'
                failures.checkpoint('automatic-startup')
            finally:
                processes.terminate(process)
                log.flush()
                print(startup_log_path.read_text(), flush=True)
        startup_output = startup_log_path.read_text()
        assert 'startup focus:' in startup_output, startup_output
        assert 'native GPU adapter:' in startup_output, startup_output
        assert 'GPU frames;' in startup_output, 'ordinary player did not finish GPU presentation normally'
        startup_rendered = int(startup_output.split('rendered ')[-1].split(' GPU frames;')[0])
        assert startup_rendered > 0, startup_output
        startup_evidence = {'launch': 'direct binary, --trace-focus --inspect (read-only)',
                            'initial_tick': initial_tick, 'later_tick': startup_state['session_tick'],
                            'gpu_frames': startup_rendered, 'resume_requested': False,
                            'focus_trace': [line for line in startup_output.splitlines()
                                            if line.startswith('startup focus:')]}
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
                route = ['right'] * 8 + ['up'] * 20 + ['right'] * 16
                for frame, action in enumerate(route, 1):
                    call('input', frame, '--actions', json.dumps({action: {'kind': 'button', 'value': True}}))
                call('step', 44)
                expected = state()
                assert expected['completed'] and expected['position'] == {'x': 3000, 'z': -2000}, expected
                win_capture = capture('win')
                assert win_capture['checksum'] != initial_capture['checksum']
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
                    # Native focus loss pauses by design; another acceptance
                    # browser may take focus while this explicit replay runs.
                    if playback()['paused']:
                        invoke('resume')
                    time.sleep(.02)
                actual = state()
                for key in ['position', 'collected', 'completed', 'remaining', 'session_tick']:
                    assert actual[key] == expected[key], (key, actual, expected)
                assert playback()['paused']
                assert capture('replay')['checksum'] == win_capture['checksum']
                call('step', 1, success=False)
                invoke('restart')
                assert state()['session_tick'] == 0 and playback()['position'] == 0
                reset_capture = capture('reset')
                assert reset_capture['checksum'] == initial_capture['checksum']
                assert reset_capture['identity']['session_generation'] > win_capture['identity']['session_generation']
                for name, x, z in [('depth-behind', 0, -1000), ('depth-front', 0, 1500), ('projection-far', -3000, -3000)]:
                    before = call('status')
                    changed = invoke('teleport', {'x': x, 'z': z})
                    assert changed['observed_frame'] == before['observed_frame']
                    assert changed['state_revision'] > before['state_revision']
                    fresh = capture(name)
                    assert fresh['identity']['state_revision'] == changed['state_revision']
                    assert fresh['checksum'] != reset_capture['checksum']
                assert captures['depth-behind']['checksum'] != captures['depth-front']['checksum']
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
                rejected = call('invoke', 'teleport', '--arguments', '{"x":0,"z":1500}', success=False)
                assert rejected['error']['code'] == 'mutation_disabled'
                assert capture('readonly')['checksum'] == initial_capture['checksum']
            finally:
                processes.terminate(process)
        summary = {'automatic_startup': startup_evidence, 'native_gpu_frames': rendered, 'replay_ticks': 44,
                          'semantic_equivalence': True, 'completed_state': actual, 'capture_registered': True, 'readonly_capture': True, 'surface_lifecycle_verified': True, 'captures': captures}
        (Path(directory) / 'evidence.json').write_text(json.dumps(sanitize(summary), indent=2))
        print(json.dumps({**summary, 'evidence_directory': directory}))
    finally:
        # Retain only bounded sanitized text and known PNGs, never discovery data.
        for path in [log_path, startup_log_path]:
            if path.exists():
                path.write_text(sanitize(path.read_text()[-128 * 1024:]))
                path.chmod(0o600)


if __name__ == '__main__':
    with FailureEvidence('collection-room-player', repo=REPO) as failures:
        main(failures)
