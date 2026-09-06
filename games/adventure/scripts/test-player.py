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
    # Two room routes plus replay/captures outlive the generic 60-second host
    # limit on CI. Preserve an explicit caller limit and owned-process cleanup.
    os.environ.setdefault("TITAN_RUNTIME_TIMEOUT_SECONDS", "240")
    processes.run(['cargo', 'build', '--locked', '--manifest-path', str(GAME / 'Cargo.toml'),
                   '--features', 'player', '--bin', 'play'], phase='build', check=True)
    processes.run(['cargo', 'build', '--locked', '-p', 'titan-cli'], cwd=REPO, phase='build', check=True)

    def target(root):
        result = processes.run(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1',
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
                                       '--run-for-ms', '240000'], project=GAME, instance=instance,
                                      cwd=GAME, stdout=log, stderr=log)
            failures.record_process(process)
            try:
                deadline = time.monotonic() + 15
                while not any(item['instance_id'] == instance for item in call('instances')['instances']):
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
                start_capture = capture('start')
                invoke('select_room', {'room': 1})
                initial_capture = capture('initial')
                assert capture('initial-repeat')['checksum'] == initial_capture['checksum']
                route = json.loads((GAME / 'tests/control-route.json').read_text())
                for frame, sample in enumerate(route, 1):
                    call('input', frame, '--actions', json.dumps({action: {'kind': 'button', 'value': True} for action in sample['actions']}))
                call('step', len(route))
                expected = state()
                assert expected['active_character'] == route[-1]['active_character'], expected
                for name, position in route[-1]['characters'].items():
                    assert all(expected['characters'][name][axis] == value for axis, value in position.items()), expected
                win_capture = capture('moved')
                assert win_capture['checksum'] != initial_capture['checksum']
                recording = call('query', 'recording')['response']['value']
                invoke('load_replay', {'recording': recording})
                assert playback()['position'] == 0
                call('step', 1)
                assert all(state()['characters']['jumper'][axis] == value for axis, value in {'x': 1560, 'y': 0, 'z': 6500}.items())
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
                invoke('restart')
                def move(actions, ticks):
                    frame = call('status')['response']['current_frame']
                    for offset in range(1, ticks + 1):
                        call('input', frame + offset, '--actions', json.dumps({a: {'kind': 'button', 'value': True} for a in actions}))
                    call('step', ticks)
                move(['up'], 50)
                move(['up', 'jump'], 17)
                assert state()['characters']['jumper']['y'] == 1530
                capture('jump-apex')
                move(['up', 'jump'], 5)
                move([], 14)
                assert state()['characters']['jumper']['y'] == 1000 and state()['characters']['jumper']['grounded']
                capture('ledge-landed')
                move(['down'], 30)
                move([], 20)
                assert state()['characters']['jumper']['y'] == 0 and state()['characters']['jumper']['grounded']
                invoke('restart')
                invoke('switch')
                move(['left'], 25)
                move(['up'], 50)
                move(['up', 'jump'], 9)
                assert state()['characters']['strong']['y'] == 450
                capture('strong-apex')
                move(['up', 'jump'], 31)
                assert state()['characters']['strong']['y'] == 0 and state()['characters']['strong']['z'] == 3200
                capture('strong-blocked')
                invoke('restart')
                solution = json.loads((GAME / 'tests/puzzle-solution.json').read_text())
                for segment in solution:
                    move(segment['actions'], segment['ticks'])
                    checkpoint = segment.get('checkpoint')
                    if checkpoint:
                        puzzle = state()['puzzle']
                        if checkpoint == 'plate-a':
                            assert puzzle['plates'][0]['pressed'] and puzzle['door']['state'] == 'open_plate'
                        elif checkpoint == 'plate-b':
                            assert all(p['pressed'] for p in puzzle['plates'])
                        elif checkpoint == 'exchange':
                            assert not puzzle['plates'][0]['pressed'] and puzzle['plates'][1]['pressed'] and puzzle['door']['open']
                        elif checkpoint == 'jumper-exit':
                            assert puzzle['exit']['jumper'] and not puzzle['exit']['strong'] and not puzzle['complete']
                        elif checkpoint == 'complete':
                            assert puzzle['complete'] and all(puzzle['exit'].values()) and puzzle['door']['state'] == 'closed'
                        capture(f'puzzle-{checkpoint}')
                solved = state()
                solution_recording = call('query', 'recording')['response']['value']
                move(['left', 'jump', 'switch'], 10)
                frozen = state()
                for key in ('session_tick', 'characters', 'active_character', 'puzzle'):
                    assert frozen[key] == solved[key], key
                # A JSON file avoids platform argument-size limits for the full route.
                arguments = Path(directory) / 'solution-replay.json'
                arguments.write_text(json.dumps({'recording': solution_recording}))
                call('invoke', 'load_replay', '--arguments-file', arguments)
                call('step', len(solution_recording['frames']))
                assert state()['puzzle']['complete'] and state()['characters'] == solved['characters']
                assert playback()['complete'] and playback()['paused']
                capture('puzzle-solution-replay')
                invoke('restart')
                puzzle = state()['puzzle']
                assert not puzzle['complete'] and not puzzle['door']['open'] and not any(p['pressed'] for p in puzzle['plates'])
                capture('puzzle-restarted')
                # Ordinary input also produces the hold-open obstruction reason.
                for segment in solution[:4]:
                    move(segment['actions'], segment['ticks'])
                move(['switch'], 1)
                move([], 1)
                move(['up'], 25)
                move(['right'], 67)
                move(['switch'], 1)
                move([], 1)
                move(['down'], 50)
                assert state()['puzzle']['door']['state'] == 'open_obstructed'
                capture('puzzle-obstructed')
                move(['switch'], 1)
                move([], 1)
                move(['right'], 15)
                assert state()['puzzle']['door']['state'] == 'closed'
                capture('puzzle-cleared')
                invoke('select_room', {'room': 2})
                move(['interact', 'up'], 1)
                assert state()['block']['last_rejection'] == 'wrong_character'
                assert state()['block']['socket'] == 0
                capture('block-rejected')
                for route_name in ('block-solution.json', 'block-intermediate-solution.json'):
                    invoke('select_room', {'room': 2})
                    assert state()['room'] == 2
                    capture(f'{route_name}-initial')
                    for segment in json.loads((GAME / 'tests' / route_name).read_text()):
                        move(segment['actions'], segment['ticks'])
                        if segment.get('checkpoint'):
                            capture(f"{route_name}-{segment['checkpoint']}")
                    solved_block = state()
                    assert solved_block['puzzle']['complete'], solved_block
                    block_recording = call('query', 'recording')['response']['value']
                    invoke('confirm')
                    assert state()['room'] == 1 and state()['phase'] == 'playing'
                    capture(f'{route_name}-play-again')
                    arguments.write_text(json.dumps({'recording': block_recording}))
                    call('invoke', 'load_replay', '--arguments-file', arguments)
                    call('step', len(block_recording['frames']))
                    assert state()['room'] == 2 and state()['puzzle']['complete']
                    assert state()['characters'] == solved_block['characters']
                    invoke('restart')
                    assert state()['room'] == 2 and not state()['puzzle']['complete']
                capture('block-reset')
                # Replay the complete Start-origin sequence on the actual GPU host.
                sequence_segments = json.loads((GAME / 'tests/sequence-solution.json').read_text())
                sequence_recording = dict(block_recording, room=1,
                    origin={'phase': 'start', 'blocked_actions': [], 'recovery_message_ticks': 0}, frames=[])
                held = set()
                for segment in sequence_segments:
                    for _ in range(segment['ticks']):
                        active = set(segment['actions'])
                        sequence_recording['frames'].append({'active': sorted(active), 'pressed': sorted(active-held), 'released': sorted(held-active)})
                        held = active
                arguments.write_text(json.dumps({'recording': sequence_recording}))
                call('invoke', 'load_replay', '--arguments-file', arguments)
                assert state()['phase'] == 'start'
                capture('sequence-start')
                previous_generation = state()['session_generation']
                for segment in sequence_segments:
                    call('step', segment['ticks'])
                    checkpoint = segment.get('checkpoint')
                    if checkpoint in ('started', 'continued'):
                        current = state()
                        assert current['phase'] == 'playing' and current['session_tick'] == 0 and current['active_character'] == 'jumper'
                        assert current['session_generation'] == previous_generation + 1
                        previous_generation = current['session_generation']
                        capture(f'sequence-{checkpoint}')
                    elif checkpoint == 'complete':
                        current = state()
                        assert current['phase'] == ('room_complete' if current['room'] == 1 else 'slice_complete')
                        capture(f"sequence-room-{current['room']}-complete")
                assert playback()['complete'] and state()['phase'] == 'slice_complete'
                invoke('restart')
                assert state()['room'] == 2 and state()['phase'] == 'playing'
                capture('sequence-restart-room')
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
                while not any(item['instance_id'] == instance for item in call('instances')['instances']):
                    assert process.poll() is None, 'read-only GPU host exited'
                    assert time.monotonic() < deadline, 'read-only discovery timed out'
                    time.sleep(.05)
                time.sleep(.5)
                assert not call('capabilities')['response']['mutation_enabled']
                rejected = call('invoke', 'switch', '--arguments', '{}', success=False)
                assert rejected['error']['code'] == 'mutation_disabled'
                assert capture('readonly')['checksum'] == start_capture['checksum']
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
