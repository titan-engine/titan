#!/usr/bin/env python3
"""One bounded factory evaluation. Run at repo root; only scratch source is edited."""
import datetime
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import tempfile
import time

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes


def main():
    report = {'schema': 1, 'game': 'factory', 'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
              'environment': {'os': platform.platform(), 'python': platform.python_version(),
                              'cpu': subprocess.check_output(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip() if platform.system() == 'Darwin' else platform.processor(),
                              'logical_cpus': os.cpu_count(), 'profile': 'Cargo dev (unoptimized + debuginfo)',
                              'backend': 'headless native control; software renderer',
                              'concurrency': 'private target; global registry/cache and host CPU shared with other agents/builds',
                              'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
                              'cargo': subprocess.check_output(['cargo', '--version'], text=True).strip()},
              'cache': 'empty private CARGO_TARGET_DIR; existing global Cargo registry/toolchain cache retained',
              'boundary': 'automated operation start through asserted result; excludes reading docs, authoring harness, scratch extraction and human/agent reasoning',
              'human_interventions': 0, 'phases': [], 'unexpected_failures': []}
    with tempfile.TemporaryDirectory(prefix='titan-factory-eval-') as tmp:
        root = Path(tmp)
        archive = subprocess.check_output(['git', 'archive', report['revision']], cwd=REPO)
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(root)
        game = root / 'games/factory'
        env = dict(os.environ, CARGO_TARGET_DIR=str(root / 'build'))
        binary, cli = root / 'build/debug/titan-factory', root / 'build/debug/titan'
        instance = 'factory-baseline-' + str(os.getpid())
        def run(args, phase='runtime'):
            result = processes.run([str(a) for a in args], cwd=game, env=env, phase=phase, capture_output=True, text=True)
            if result.returncode:
                raise RuntimeError('command exited ' + str(result.returncode) + ': ' + result.stderr[-2000:].replace(str(root), '<scratch>'))
            return result.stdout
        def phase(name, fn):
            start = time.perf_counter()
            value = fn()
            report['phases'].append({'name': name, 'seconds': round(time.perf_counter()-start, 6), 'attempts': 1, 'result': value})
            print(name + ' verified', file=sys.stderr, flush=True)
            return value
        def build():
            run(['cargo', 'build', '--locked', '--manifest-path', root/'Cargo.toml', '-p', 'titan-cli'], 'build')
            run(['cargo', 'build', '--locked', '--manifest-path', game/'Cargo.toml', '--bin', 'titan-factory'], 'build')
            return {'cli_and_game_binaries_exist': cli.is_file() and binary.is_file()}
        phase('build_cold_private_target', build)
        def call(*args, error=None):
            result = processes.run([str(cli), '--format', 'json', '--project', str(game), '--instance', instance, *args], cwd=game, env=env, capture_output=True, text=True)
            data = json.loads(result.stdout)
            if error:
                assert result.returncode != 0 and data['error']['code'] == error, data
            else:
                assert result.returncode == 0 and data['status'] == 'success', data
            return data
        def state():
            return call('query', 'state')['response']['value']
        def invoke(name, value, **kw):
            return call('invoke', name, '--arguments', json.dumps(value), **kw)
        start = time.perf_counter()
        with open(root/'runtime.log', 'w') as log:
            process = processes.Popen([str(binary), '--serve', '--instance', instance, '--allow-mutation', '--run-for-ms', '120000'], cwd=game, env=env, stdout=log, stderr=log, project=game, instance=instance)
            try:
                polls = 0
                while True:
                    polls += 1
                    if call('instances')['instances']:
                        break
                    assert time.perf_counter()-start < 10 and process.poll() is None
                    time.sleep(.05)
                assert len(state()['structures']) == 1
                report['phases'].append({'name': 'launch_discovery_initial_state', 'seconds': round(time.perf_counter()-start,6), 'attempts': 1, 'polls': polls, 'result': {'structures': 1, 'tick': 0}})
                def inspect():
                    caps = call('capabilities')['response']
                    commands = call('commands')['response']
                    entities = call('entities')['response']
                    return {'capabilities': caps, 'commands': commands, 'entities': entities}
                phase('capability_command_entity_inspection', inspect)
                ops = [{'op':'place','kind':kind,'x':x,'y':3,'facing':'E'} for kind,x in [('extractor',1),('conveyor',2),('processor',3)]] + [{'op':'advance','ticks':60}]
                sequence = game/'baseline-scenario.json'
                def scenario():
                    sequence.write_text(json.dumps(ops))
                    invoke('sequence', {'operations':ops})
                    s = state()
                    assert s['tick'] == 60 and s['delivered'] == 0 and len(s['structures']) == 4
                    recording = call('query', 'recording')['response']['value']
                    assert recording['dropped'] == 0 and len(recording['operations']) == 4
                    return {'state': s, 'recording': recording}
                built = phase('construct_scenario_write_control_verify', scenario)['state']
                def replay():
                    replayed = json.loads(run([binary, '--sequence', sequence]))
                    assert all('error' not in op for op in replayed['outcomes'])
                    assert replayed['state'] == built
                    return {'exact_full_state_equal': True, 'operations':4}
                phase('replay_fresh_process_verify', replay)
                def capture():
                    a = call('capture')['response']
                    data = Path(a['artifact']).read_bytes()
                    assert data.startswith(b'P6\n384 256\n255\n') and len(data) == 294927
                    b = call('capture')['response']
                    assert a['checksum'] == b['checksum']
                    return {k:v for k,v in a.items() if k != 'artifact'} | {'ppm_bytes':len(data), 'repeat_checksum_equal':True}
                phase('software_capture_read_repeat_verify', capture)
                def diagnose():
                    before = state()
                    failure = invoke('remove', {'x':10,'y':3}, error='invalid_value')['error']
                    assert 'FIXED' in failure['message']
                    assert state() == before
                    tile = call('query', 'tile', '--arguments', '{"x":10,"y":3}')['response']['value']
                    assert tile['structure']['kind'] == 'delivery'
                    manifest = Path(failure['details']['diagnostic_bundle'])
                    bundle = json.loads(manifest.read_text())
                    assert (manifest.parent/'api.txt').is_file() and (manifest.parent/'capture.png').is_file()
                    # Correct the target: the conveyor is removable; delivery is fixed.
                    invoke('remove', {'x':2,'y':3})
                    after = state()
                    empty = call('query', 'tile', '--arguments', '{"x":2,"y":3}')['response']['value']
                    delivery = call('query', 'tile', '--arguments', '{"x":10,"y":3}')['response']['value']
                    assert empty['structure'] is None and delivery == tile
                    assert after['structures'] == [v for v in before['structures'] if (v['x'],v['y']) != (2,3)]
                    assert after['tick'] == before['tick'] == 60
                    return {'recovery':{'corrected_target':{'x':2,'y':3},'removed_conveyor':True,'delivery_unchanged':True,'tick':after['tick'],'structure_count':len(after['structures'])}, 'expected_error':failure['code'], 'message':failure['message'], 'state_unchanged':True, 'tile':tile, 'diagnostic_manifest_api_png_exist':True, 'manifest_keys':sorted(bundle)}
                phase('diagnose_fixed_delivery_rejection_and_recover', diagnose)
            finally:
                processes.terminate(process)
        def cleanup():
            assert not call('instances')['instances']
            return {'registration_removed':True, 'process_exited':process.poll() is not None}
        phase('shutdown_verify', cleanup)
        def change_rule():
            seq = game/'rule-probe.json'
            seq.write_text(json.dumps([{'op':'advance','ticks':61},{'op':'advance','ticks':60}]))
            baseline = json.loads(run([binary,'--sequence',seq]))
            assert all('error' not in op for op in baseline['outcomes']) and baseline['state']['tick'] == 121
            source = game/'src/game.rs'
            text = source.read_text()
            old = 'if ticks > 36000 {'
            assert text.count(old) == 1
            source.write_text(text.replace(old, 'if ticks > 60 {').replace('at most 36000 ticks', 'at most 60 ticks').replace('fixed ticks, 0..36000', 'fixed ticks, 0..60'))
            run(['cargo','build','--locked','--manifest-path',game/'Cargo.toml','--bin','titan-factory'], 'build')
            seq = game/'rule-probe.json'
            seq.write_text(json.dumps([{'op':'advance','ticks':61},{'op':'advance','ticks':60}]))
            result = json.loads(run([binary,'--sequence',seq]))
            assert result['outcomes'][0]['error'] == 'ADVANCE_LIMIT: use at most 60 ticks per operation'
            assert 'error' not in result['outcomes'][1] and result['state']['tick'] == 60
            return {'baseline_accepts_61_and_60':True,'baseline_tick':121,'scratch_only':True,'new_limit':60,'rejects_61':True,'accepts_60':True,'final_tick':60}
        phase('change_rule_baseline_edit_incremental_build_verify', change_rule)
    output = REPO/'docs/evidence/agent-iteration/factory-measurement.json'
    output.write_text(json.dumps(report, indent=2)+'\n')
    print(str(output.relative_to(REPO)))

if __name__ == '__main__':
    main()
