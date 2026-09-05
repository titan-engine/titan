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
        assert data['error']['code'] == error, data
    else:
        assert data['status'] == 'success', data
    return data


def scenario(control):
    instance = f'live-player-{os.getpid()}-{int(control)}'
    process = subprocess.Popen([
        str(BINARY), '--inspect', '--instance', instance, '--run-for-ms', '15000',
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
        resumed = call(instance, 'invoke', 'resume')
        time.sleep(.2)
        paused_again = call(instance, 'invoke', 'pause')
        assert paused_again['observed_frame'] > resumed['observed_frame'], paused_again
        assert paused_again['state_revision'] > resumed['state_revision'], paused_again
        state = call(instance, 'query', 'arena_state')['response']['value']
        assert state['paused'] and state['run']['dash_cooldown'] > 0, state
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
print('Native GPU live-player read-only policy, safe-point pause, stable inspection, dash step, capture, resume, live contact headless replay and registration cleanup passed.')
