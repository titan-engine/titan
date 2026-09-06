#!/usr/bin/env python3
"""Run in an already patched disposable checkout; never edits shipped gameplay.

python3 reproduce.py SCRATCH_CHECKOUT OUTPUT_DIRECTORY
Build target is private to SCRATCH_CHECKOUT; existing toolchain/registry retained.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
OUT.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(ROOT / 'scripts'))
import acceptance_process as processes

GAME = ROOT / 'games/factory'
TARGET = ROOT / 'variation-build'
ENV = dict(os.environ, CARGO_TARGET_DIR=str(TARGET))
CLI = TARGET / 'debug/titan'
BIN = TARGET / 'debug/titan-factory'
INSTANCE = 'factory-variation-' + str(os.getpid())
REPORT = {'revision': subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT, text=True).strip(),
          'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'environment': {'os': platform.platform(), 'python': platform.python_version(),
                          'cpu': subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'], text=True).strip(),
                          'logical_cpus': os.cpu_count(),
                          'rustc': subprocess.check_output(['rustc','--version'], text=True).strip(),
                          'cargo': subprocess.check_output(['cargo','--version'], text=True).strip(),
                          'node': subprocess.check_output(['node','--version'], text=True).strip()},
          'private_target_initially_exists': TARGET.exists(), 'phases': [], 'failures': []}

def run(args, phase='runtime'):
    result = processes.run([str(a) for a in args], cwd=GAME, env=ENV, phase=phase, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr[-4000:].replace(str(ROOT), '<scratch>'))
    return result.stdout

def phase(name, action):
    start = time.perf_counter()
    try:
        value = action()
    except Exception as error:
        REPORT['failures'].append({'phase': name, 'error': str(error).replace(str(ROOT), '<scratch>')})
        raise
    finally:
        REPORT['phases'].append({'name':name, 'seconds': round(time.perf_counter()-start,6)})
        (OUT/'measurement.json').write_text(json.dumps(REPORT,indent=2)+'\n')
    print(name + ' verified', flush=True)
    return value

def call(*args, error=None):
    p = processes.run([str(CLI),'--format','json','--project',str(GAME),'--instance',INSTANCE,*args], cwd=GAME, env=ENV, capture_output=True,text=True)
    data = json.loads(p.stdout)
    if error:
        assert p.returncode != 0 and data['error']['code'] == error, data
    else:
        assert p.returncode == 0 and data['status'] == 'success', data
    return data

def state():
    return call('query','state')['response']['value']

def invoke(name, args, **kw):
    return call('invoke',name,'--arguments',json.dumps(args),**kw)

def conserved(s):
    resident = sum(item is not None for structure in s['structures'] for item in structure['slots'].values())
    assert s['seeded'] + s['extracted'] == resident + s['delivered'] + s['discarded_ore'] + s['discarded_plate'], s
    return resident

def game_state(s):
    # The documented host clock survives restart and advances while Complete.
    return {k:v for k,v in s.items() if k != 'frame'}

route = [{'op':'place','kind':'extractor' if x==1 else 'processor' if x==5 else 'conveyor','x':x,'y':3,'facing':'E'} for x in range(1,10)]
operations = route + [{'op':'advance','ticks':1} for _ in range(969)]
(OUT/'route.json').write_text(json.dumps(operations,indent=2)+'\n')

def build():
    run(['cargo','build','--locked','--manifest-path',ROOT/'Cargo.toml','-p','titan-cli'],'build')
    run(['cargo','build','--locked','--manifest-path',GAME/'Cargo.toml','--bin','titan-factory'],'build')

phase('build',build)

def trace():
    data = json.loads(run([BIN,'--sequence',OUT/'route.json']))
    replay = json.loads(run([BIN,'--sequence',OUT/'route.json']))
    assert replay == data
    checkpoints = []
    for outcome in data['outcomes']:
        assert 'error' not in outcome, outcome
        s = outcome['state']
        conserved(s)
        tick = s['tick']
        expected = 0 if tick < 159 else min(10, 1+(tick-159)//90)
        assert s['delivered'] == expected, (tick,s['delivered'],expected)
        if tick in [64,153,154,158,159,249,969]:
            processor = next(st for st in s['structures'] if st['x']==5)
            assert processor['recipe']['total'] == 90
            if tick == 64:
                assert processor['remaining'] == 90
                assert processor['recipe']['elapsed'] == 0
            checkpoints.append(s)
    assert data['state']['completion_tick'] == 969
    REPORT['trace'] = {'asserted_boundaries': len(data['outcomes']), 'fresh_process_same_order_all_outcomes_equal':True,'completion_tick':969,'first_delivery_tick':159,'delivery_interval':90,'checkpoints':checkpoints}
    return data['state']

expected = phase('full_route_each_tick_conservation',trace)

def exercise_live():
    log = open(ROOT/'variation-runtime.log','w')
    process = processes.Popen([str(BIN),'--serve','--instance',INSTANCE,'--allow-mutation','--run-for-ms','120000'],cwd=GAME,env=ENV,stdout=log,stderr=log,project=GAME,instance=INSTANCE)
    try:
        start = time.perf_counter()
        while not call('instances')['instances']:
            assert process.poll() is None and time.perf_counter()-start<10
            time.sleep(.05)
        before = state()
        assert before['tick']==0 and len(before['structures'])==1
        REPORT['discovery'] = {name:call(name)['response'] for name in ['capabilities','commands','queries','entities']}
        failure = invoke('remove',{'x':10,'y':3},error='invalid_value')['error']
        assert 'FIXED' in failure['message']
        assert state()==before
        manifest = Path(failure['details']['diagnostic_bundle'])
        bundle = json.loads(manifest.read_text())
        assert (manifest.parent/'api.txt').is_file()
        assert (manifest.parent/'capture.png').is_file()
        # Correct the target: a temporary conveyor can be placed then removed.
        invoke('place',{'kind':'conveyor','x':2,'y':3,'facing':'E'})
        invoke('remove',{'x':2,'y':3})
        assert state()==before
        REPORT['diagnostic'] = {'code':failure['code'],'message':failure['message'], 'unchanged_state':True,'corrected_conveyor_removed':True,'manifest_keys':sorted(bundle),'api_and_capture_exist':True}
        invoke('sequence',{'operations':route+[{'op':'advance','ticks':969}]})
        assert state()==expected
        first = call('capture')['response']
        pixels = Path(first.pop('artifact')).read_bytes()
        assert pixels.startswith(b'P6\n384 256\n255\n')
        (OUT/'complete.ppm').write_bytes(pixels)
        invoke('advance',{'ticks':10})
        assert game_state(state())==game_state(expected)
        invoke('restart',{})
        assert game_state(state())==game_state(before)
        invoke('sequence',{'operations':route+[{'op':'advance','ticks':969}]})
        assert game_state(state())==game_state(expected)
        second = call('capture')['response']
        replay_pixels = Path(second.pop('artifact')).read_bytes()
        assert pixels==replay_pixels and first['checksum']==second['checksum']
        REPORT['capture'] = {'first':first,'replay':second,'artifact':'complete.ppm','bytes':len(pixels),'sha256':hashlib.sha256(pixels).hexdigest(),'identical_bytes':True}
        REPORT['replay'] = {'fresh_process_vs_trace_full_state_equal':True,'restart_replay_all_game_fields_equal_except_host_frame':True,'freeze_verified_except_host_frame':True}
    finally:
        processes.terminate(process)
        log.close()
    assert not call('instances')['instances']
    REPORT['cleanup'] = {'owned_process_exited':process.poll() is not None,'registration_removed':True}

phase('live_discovery_diagnosis_recovery_replay_capture_cleanup',exercise_live)
