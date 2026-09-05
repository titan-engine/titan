#!/usr/bin/env python3
"""Exercise the real GPU player's authenticated live inspection on a desktop."""
import json
import os
from pathlib import Path
import subprocess
import time

GAME = Path(__file__).resolve().parents[1]
REPO = GAME.parents[1]


def target(manifest):
    return Path(json.loads(subprocess.check_output([
        'cargo', 'metadata', '--no-deps', '--format-version', '1',
        '--manifest-path', str(manifest),
    ]))['target_directory'])


subprocess.run(['cargo', 'build', '--manifest-path', str(GAME / 'Cargo.toml'), '--bin', 'play', '--bin', 'replay'], check=True)
subprocess.run(['cargo', 'build', '--manifest-path', str(REPO / 'Cargo.toml'), '-p', 'titan-cli'], check=True)
BINARY = target(GAME / 'Cargo.toml') / 'debug/play'
CLI = target(REPO / 'Cargo.toml') / 'debug/titan'


def call(instance, *args, error=None):
    result = subprocess.run([
        str(CLI), '--format', 'json', '--project', str(GAME), '--instance', instance,
        *map(str, args),
    ], capture_output=True, text=True, timeout=10)
    data = json.loads(result.stdout)
    if error:
        assert data['status'] == 'failure', data
        if error is not True:
            assert data['error']['code'] == error, data
    else:
        assert data['status'] == 'success', data
    return data


def scenario(control):
    instance = f'live-player-{os.getpid()}-{int(control)}'
    process = subprocess.Popen([
        str(BINARY), '--inspect', '--instance', instance, '--run-for-ms', '30000',
        *(['--allow-control'] if control else []),
    ], cwd=GAME, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        deadline = time.monotonic() + 10
        while True:
            assert process.poll() is None, 'Native GPU player exited before registration'
            instances = call(instance, 'instances')['instances']
            if any(item['instance_id'] == instance for item in instances):
                break
            assert time.monotonic() < deadline, 'Native GPU player did not register'
            time.sleep(.05)
        capabilities = call(instance, 'capabilities')['response']
        assert capabilities['run_mode'] == 'interactive', capabilities
        assert capabilities['mutation_enabled'] == control, capabilities
        if not control:
            call(instance, 'invoke', 'pause', error='mutation_disabled')
            call(instance, 'step', 1, error='mutation_disabled')
            call(instance, 'invoke', 'restart', error='mutation_disabled')
            readonly_save = call(instance, 'query', 'save')['response']['value']
            call(instance, 'invoke', 'load_save', '--arguments', json.dumps({'save': readonly_save}), error='mutation_disabled')
            call(instance, 'capture')
            return
        paused = call(instance, 'invoke', 'pause')
        frame = paused['observed_frame']
        time.sleep(.2)
        stable = call(instance, 'status')
        assert stable['observed_frame'] == frame, (paused, stable)
        assert stable['state_revision'] == paused['state_revision'], (paused, stable)
        call(instance, 'invoke', 'restart')
        initial = call(instance, 'capture')['response']
        assert initial['checksum'] == 'e096abf94fd12c24', initial
        call(instance, 'input', frame + 1, '--actions', '{"right":{"kind":"button","value":true},"dash":{"kind":"button","value":true}}')
        stepped = call(instance, 'step', 1)
        assert stepped['observed_frame'] == frame + 1, stepped
        entities = call(instance, 'entities')['response']['entities']
        player = next(entity['id'] for entity in entities if entity['name'] == 'player')
        entity = call(instance, 'entity', player['index'], player['generation'])['response']
        position = next(value for key, value in entity['components'].items() if key.endswith('::Position'))
        assert position == {'x': 84, 'y': 65}, position
        capture = call(instance, 'capture')
        assert capture['observed_frame'] == frame + 1, capture
        assert capture['response']['checksum'] != initial['checksum'], capture
        mid_dash_save = call(instance, 'query', 'save')['response']['value']
        resumed = call(instance, 'invoke', 'resume')
        time.sleep(.2)
        paused_again = call(instance, 'invoke', 'pause')
        assert paused_again['observed_frame'] > resumed['observed_frame'], paused_again
        assert paused_again['state_revision'] > resumed['state_revision'], paused_again
        state = call(instance, 'query', 'arena_state')['response']['value']
        assert state['paused'] and state['run']['dash_cooldown'] > 0, state
        # Restore the exact paused live scene, preserving host frame/revision identity.
        saved = call(instance, 'query', 'save')['response']['value']
        saved_checksum = call(instance, 'capture')['response']['checksum']
        call(instance, 'step', 20)
        before_load = call(instance, 'status')
        rejected = call(instance, 'invoke', 'load_save', '--arguments', '{"save":{}}', error='invalid_value')
        assert (rejected['observed_frame'], rejected['state_revision']) == (before_load['observed_frame'], before_load['state_revision']), rejected
        evidence = GAME / 'target' / 'arena-evidence'
        evidence.mkdir(parents=True, exist_ok=True)
        load_arguments = evidence / 'live-load-arguments.json'
        load_arguments.write_text(json.dumps({'save': saved}))
        loaded = call(instance, 'invoke', 'load_save', '--arguments-file', load_arguments)
        assert loaded['observed_frame'] == before_load['observed_frame'], loaded
        assert loaded['state_revision'] > before_load['state_revision'], loaded
        assert call(instance, 'status')['response']['paused']
        assert call(instance, 'capture')['response']['checksum'] == saved_checksum
        assert call(instance, 'query', 'save')['response']['value'] == saved
        assert call(instance, 'query', 'recording')['response']['value']['invalid_reason'] is None
        call(instance, 'invoke', 'resume')
        call(instance, 'invoke', 'load_save', '--arguments', json.dumps({'save': saved}), error='not_controlled')
        call(instance, 'invoke', 'pause')
        # Play a snapshot-backed recording visibly in this same GPU player.
        call(instance, 'invoke', 'load_save', '--arguments', json.dumps({'save': mid_dash_save}))
        source_frame = call(instance, 'status')['observed_frame']
        for offset, action in enumerate(['left','up','up','right','down','left','up','right'], 1):
            call(instance, 'input', source_frame + offset, '--actions', json.dumps({action:{'kind':'button','value':True}}))
        call(instance, 'step', 8)
        snapshot_recording = call(instance, 'query', 'recording')['response']['value']
        snapshot_path = evidence / 'native-visible-snapshot-recording.json'
        snapshot_path.write_text(json.dumps(snapshot_recording))
        verification = json.loads(subprocess.check_output([str(BINARY.with_name('replay')), str(snapshot_path)], text=True))
        assert verification['save'] == call(instance, 'query', 'save')['response']['value']
        assert verification['checksum'] == call(instance, 'capture')['response']['checksum']
        replay_arguments = evidence / 'native-visible-replay-arguments.json'
        replay_arguments.write_text(json.dumps({'recording': snapshot_recording}))
        call(instance, 'invoke', 'load_replay', '--arguments-file', replay_arguments)
        assert call(instance, 'query', 'save')['response']['value'] == mid_dash_save
        def replay_state():
            return call(instance, 'query', 'arena_state')['response']['value']['replay']
        assert replay_state()['position'] == 0
        replay_frame = call(instance, 'status')['observed_frame']
        for arguments in [
            ('invoke', 'load_replay', '--arguments', '{"recording":{}}'),
            ('step', 9),
            ('input', replay_frame + 1, '--actions', '{"left":{"kind":"button","value":true}}'),
            ('set-field', player['index'], player['generation'], next(key for key in entity['components'] if key.endswith('::Position')), 'x', '--value', '10'),
            ('invoke', 'load_save', '--arguments', json.dumps({'save': mid_dash_save})),
            ('invoke', 'ui_pointer', '--arguments', '{"x":8,"y":12,"pressed":true}'),
        ]:
            before_rejected = call(instance, 'status')
            rejected = call(instance, *arguments, error=True)
            assert (rejected['observed_frame'], rejected['state_revision']) == (before_rejected['observed_frame'], before_rejected['state_revision'])
            assert call(instance, 'query', 'save')['response']['value'] == mid_dash_save
            assert replay_state()['position'] == 0
        call(instance, 'step', 3)
        restart_frame = call(instance, 'status')['observed_frame']
        call(instance, 'invoke', 'restart_replay')
        assert call(instance, 'status')['observed_frame'] == restart_frame
        assert call(instance, 'query', 'save')['response']['value'] == mid_dash_save
        call(instance, 'invoke', 'resume')
        replay_deadline = time.monotonic() + 3
        while not call(instance, 'status')['response']['paused']:
            assert time.monotonic() < replay_deadline, replay_state()
            time.sleep(.05)
        assert call(instance, 'status')['observed_frame'] == restart_frame + 8
        assert replay_state()['complete'] and replay_state()['verified'], replay_state()
        assert call(instance, 'query', 'save')['response']['value'] == verification['save']
        assert call(instance, 'capture')['response']['checksum'] == verification['checksum']
        time.sleep(.1)
        assert call(instance, 'status')['observed_frame'] == restart_frame + 8
        call(instance, 'invoke', 'stop_replay')
        assert not replay_state()['active']
        assert call(instance, 'capture')['response']['checksum'] == initial['checksum']
        assert call(instance, 'status')['observed_frame'] == restart_frame + 8
        call(instance, 'invoke', 'load_replay', '--arguments-file', replay_arguments)
        call(instance, 'invoke', 'restart')
        assert not replay_state()['active']
        assert call(instance, 'capture')['response']['checksum'] == initial['checksum']
        # Forward/backward seek through the real window host; updates also run paused.
        legacy_path = GAME / 'tests/fixtures/recording-v1.json'
        legacy_recording = json.loads(legacy_path.read_text())
        legacy_verification = json.loads(subprocess.check_output([str(BINARY.with_name('replay')), str(legacy_path)], text=True))
        replay_arguments.write_text(json.dumps({'recording': legacy_recording}))
        call(instance, 'invoke', 'load_replay', '--arguments-file', replay_arguments)
        def seek(position):
            call(instance, 'invoke', 'seek_replay', '--arguments', json.dumps({'position': position}))
            deadline = time.monotonic() + 3
            while replay_state()['position'] != position:
                assert time.monotonic() < deadline, replay_state()
                time.sleep(.02)
            assert call(instance, 'status')['response']['paused']
        for speed in [0.25, 4, 0.5, 2]:
            call(instance, 'invoke', 'replay_speed', '--arguments', json.dumps({'speed': speed}))
            assert replay_state()['speed'] == speed
        for arguments in [{'speed': 0}, {'speed': 8}]:
            before_invalid = call(instance, 'status')
            rejected = call(instance, 'invoke', 'replay_speed', '--arguments', json.dumps(arguments), error='invalid_value')
            assert rejected['state_revision'] == before_invalid['state_revision']
        seek(160)
        at_160 = call(instance, 'query', 'save')['response']['value']
        image_160 = call(instance, 'capture')['response']['checksum']
        seek(0)
        call(instance, 'step', 160)
        assert call(instance, 'query', 'save')['response']['value'] == at_160
        assert call(instance, 'capture')['response']['checksum'] == image_160
        seek(30)
        call(instance, 'input', call(instance, 'status')['observed_frame'] + 1,
             '--actions', '{"dash":{"kind":"button","value":true}}', error=True)
        call(instance, 'invoke', 'replay_speed', '--arguments', '{"speed":4}')
        call(instance, 'invoke', 'resume')
        deadline = time.monotonic() + 3
        while not replay_state()['complete']:
            assert time.monotonic() < deadline, replay_state()
            time.sleep(.02)
        assert replay_state()['verified']
        assert call(instance, 'query', 'save')['response']['value'] == legacy_verification['save']
        assert call(instance, 'capture')['response']['checksum'] == legacy_verification['checksum']
        seek(0)
        seek(194)
        assert replay_state()['verified']
        assert call(instance, 'query', 'save')['response']['value'] == legacy_verification['save']
        assert call(instance, 'capture')['response']['checksum'] == legacy_verification['checksum']
        # Reproduce a real continuously ticking contact, not just injected input.
        call(instance, 'invoke', 'restart')
        call(instance, 'invoke', 'resume')
        contact_deadline = time.monotonic() + 8
        while True:
            contact = call(instance, 'query', 'arena_state')['response']['value']
            if contact['run']['health'] < 3:
                break
            assert time.monotonic() < contact_deadline, contact
            time.sleep(.1)
        call(instance, 'invoke', 'pause')
        recording = call(instance, 'query', 'recording')
        assert recording['response']['value']['final_state']['run']['health'] < 3, recording
        evidence = GAME / 'target/arena-evidence'
        evidence.mkdir(parents=True, exist_ok=True)
        recording_path = evidence / 'native-live-recording.json'
        recording_path.write_text(json.dumps(recording))
        replay = subprocess.run([str(BINARY.with_name('replay')), str(recording_path)],
                                capture_output=True, text=True, timeout=10)
        assert replay.returncode == 0, (replay.stdout, replay.stderr)
        verification = json.loads(replay.stdout)
        assert verification['checksum'] == recording['response']['value']['final_checksum'], verification
    finally:
        if process.poll() is None:
            process.terminate()
        stdout, stderr = process.communicate(timeout=10)
        assert process.returncode == 0, (process.returncode, stdout, stderr)
        assert 'rendered ' in stdout and ' GPU frames;' in stdout, stdout
        assert not any(item['instance_id'] == instance for item in call(instance, 'instances')['instances'])


scenario(False)
scenario(True)
print('Native GPU live-player read-only policy, safe-point pause, stable inspection, dash step, capture, resume, save/load, snapshot playback/input isolation/EOF, forward/backward seeking and speed controls, live contact headless replay and registration cleanup passed.')
