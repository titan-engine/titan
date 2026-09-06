#!/usr/bin/env python3
"""One bounded headless adventure exercise; writes sanitized JSON to stdout.
Run from any directory: python3 docs/evidence/agent-iteration/adventure-baseline.py
Uses git HEAD, a disposable source copy and empty target; no GUI or gameplay edits.
"""
import io
import json
import os
from pathlib import Path
import platform
import sys
import subprocess
import tarfile
import tempfile
import time
from datetime import datetime, timezone

REPO = Path(__file__).resolve().parents[3]


sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes


def run(args, cwd=REPO, env=None):
    return processes.run(args, cwd=cwd, env=env, capture_output=True, text=True, phase='build', check=True).stdout


def main():
    report = {
        'schema_version': 1, 'game': 'adventure',
        'revision': run(['git', 'rev-parse', 'HEAD']).strip(),
        'started_utc': datetime.now(timezone.utc).isoformat(),
        'environment': {'os': platform.system(), 'release': platform.release(),
                        'machine': platform.machine(), 'python': platform.python_version(),
                        'rustc': run(['rustc', '--version']).strip(),
                        'cargo': run(['cargo', '--version']).strip(),
                        'cpu': run(['sysctl', '-n', 'machdep.cpu.brand_string']).strip() if platform.system() == 'Darwin' else platform.processor()},
        'cache': 'Empty isolated CARGO_TARGET_DIR; existing shared Cargo registry/download cache; no cache purge.',
        'boundaries': 'perf_counter wall seconds; each phase includes subprocess startup, JSON decoding and assertions. Preparation and script authoring excluded. Build timers end after successful cargo exit and process cleanup. No agent cognition, GUI, browser or population percentile claims. Cargo fetch time, if needed, is included in build time.',
        'human_interventions': 0, 'phases': [],
        'timeout_seconds': {key: processes.timeout_seconds(key) for key in ('build', 'runtime')},
        'unexpected_failed_attempts': 0,
    }
    def phase(name, function):
        start = time.perf_counter()
        value = function()
        report['phases'].append({'name': name, 'elapsed_seconds': round(time.perf_counter() - start, 6),
                                 'attempts': 1, 'outcome': 'verified', 'evidence': value})
        return value
    with tempfile.TemporaryDirectory(prefix='titan-adventure-baseline-') as directory:
        scratch = Path(directory)
        source = scratch / 'source'
        source.mkdir()
        archive = processes.run(['git', 'archive', report['revision']], cwd=REPO, capture_output=True, check=True).stdout
        with tarfile.open(fileobj=io.BytesIO(archive)) as contents:
            contents.extractall(source)  # Trusted local git archive; supports Python 3.9.
        game = source / 'games/adventure'
        target = scratch / 'build'
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        cli = target / 'debug/titan'
        binary = target / 'debug/titan-adventure'
        instance = 'adventure-baseline'
        def build(manifest, extra):
            output = processes.run(['cargo', 'build', '--locked', '--manifest-path', str(manifest), *extra], cwd=source, env=env, capture_output=True, text=True, phase='build', check=True)
            return {'cargo_exit': output.returncode, 'build_lock_wait_observed': 'Blocking waiting for file lock' in output.stderr}
        phase('build_cli', lambda: build(source / 'Cargo.toml', ['-p', 'titan-cli']))
        phase('build_game', lambda: build(game / 'Cargo.toml', ['--bin', 'titan-adventure']))
        def call(*args, error=None):
            result = processes.run([str(cli), '--format', 'json', '--project', str(game), '--instance', instance, *map(str, args)], cwd=source, env=env, capture_output=True, text=True)
            data = json.loads(result.stdout)
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
            frame = call('status')['response']['current_frame'] + 1
            call('input', frame, '--actions', json.dumps({a: {'kind': 'button', 'value': True} for a in actions}))
            call('step', 1)
            return state()
        process = None
        def start():
            nonlocal process
            process = processes.Popen([str(binary), '--serve', '--project', str(game), '--instance', instance, '--run-for-ms', '120000'], project=game, instance=instance, cwd=game, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            deadline = time.monotonic() + 10
            polls = 0
            while True:
                polls += 1
                found = call('instances')['instances']
                if found:
                    assert len(found) == 1
                    initial = state()
                    assert initial['session_tick'] == 0 and initial['active_character'] == 'jumper'
                    return {'discovery_polls': polls, 'initial_characters': initial['characters']}
                assert process.poll() is None and time.monotonic() < deadline
                time.sleep(.025)
        def stop():
            nonlocal process
            if process:
                try:
                    processes.graceful_shutdown(process)
                    assert not call('instances')['instances']
                finally:
                    processes.terminate(process)
                    process = None
        try:
            phase('launch_discover_initial_state', start)
            def inspect():
                capabilities = call('capabilities')['response']
                entities = call('entities')['response']['entities']
                for name in ('jumper', 'strong'):
                    entity = next(item for item in entities if item['name'] == name)
                    detail = call('entity', entity['id']['index'], entity['id']['generation'])['response']
                    key = next(key for key in detail['components'] if key.endswith('::Position'))
                    assert all(not detail['component_fields'][key][axis]['writable'] for axis in ('x', 'z'))
                return {'capabilities': capabilities, 'read_only_character_positions': True}
            phase('discover_capabilities_inspect_entities', inspect)
            def scenario():
                # Move Jumper, switch with held right, release, move Strong north.
                samples = [drive(a) for a in [['right'], ['right', 'switch'], [], ['up']]]
                assert samples[0]['characters']['jumper'] == {'x': 1560, 'z': 6500}
                assert samples[1]['characters'] == samples[0]['characters']
                final = samples[-1]
                assert final['characters'] == {'jumper': {'x': 1560, 'z': 6500}, 'strong': {'x': 3500, 'z': 6440}}
                assert final['active_character'] == 'strong'
                return {'action_snapshots': [['right'], ['right', 'switch'], [], ['up']], 'state': final}
            phase('construct_two_character_scenario', scenario)
            recording = call('query', 'recording')['response']['value']
            def replay():
                before = state()
                invoke('replay', {'recording': recording})
                after = state()
                keys = ('characters', 'active_character', 'session_tick', 'consumed_input')
                assert all(before[k] == after[k] for k in keys)
                return {'recording': recording, 'compared_fields': keys, 'equal': True}
            phase('replay_scenario', replay)
            def failure():
                before = state()
                response = invoke('replay', {'recording': {**recording, 'fixture': 'wrong'}}, error='invalid_value')
                assert state() == before
                path = Path(response['error']['details']['diagnostic_bundle'])
                manifest = json.loads(path.read_text())
                assert manifest['request']['request']['arguments']['recording']['fixture'] == 'wrong'
                assert manifest['world_state']['game'] == before
                assert manifest['capture'] is None
                assert (path.parent / 'api.txt').is_file()
                invoke('replay', {'recording': recording})
                recovered = state()
                recovery_keys = ('characters', 'active_character', 'session_tick', 'consumed_input')
                assert all(recovered[key] == before[key] for key in recovery_keys)
                return {'error_code': response['error']['code'], 'message': response['error']['message'],
                        'diagnostic_manifest_read': True, 'diagnostic_request_and_state_verified': True,
                        'api_summary_present': True, 'diagnostic_capture': None,
                        'valid_recording_recovery_verified': True, 'recovery_compared_fields': recovery_keys, 'manifest_keys': sorted(manifest), 'state_unchanged': True,
                        'cause': 'Recording fixture identity was deliberately changed to wrong; replay rejects before restart.'}
            phase('diagnose_invalid_recording', failure)
            def capture():
                response = call('capture', error='unsupported')
                return {'error_code': response['error']['code'], 'message': response['error']['message'], 'image_produced': False}
            phase('cpu_capture_rejection', capture)
            stop()
            def rule():
                path = game / 'src/game.rs'
                before = path.read_text()
                old = 'pub const AXIAL_STEP: i32 = 60;'
                assert before.count(old) == 1
                path.write_text(before.replace(old, 'pub const AXIAL_STEP: i32 = 90;'))
                built = build(game / 'Cargo.toml', ['--bin', 'titan-adventure'])
                start()
                changed = drive(['right'])
                assert changed['characters'] == {'jumper': {'x': 1590, 'z': 6500}, 'strong': {'x': 3500, 'z': 6500}}
                return {'scratch_diff': 'AXIAL_STEP: 60 -> 90 mm/tick; diagonal unchanged', 'state': changed, **built}
            phase('change_rule_edit_build_launch_control_assert', rule)
        finally:
            stop()
        report['cleanup'] = 'Owned hosts stopped; discovery empty; disposable source and build deleted on exit.'
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
