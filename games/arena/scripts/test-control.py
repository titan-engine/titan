#!/usr/bin/env python3
"""Bounded standalone native CLI acceptance; run from any directory."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "scripts"))
import acceptance_process as processes
import time

GAME = Path(__file__).resolve().parents[1]
REPO = GAME.parents[1]
started=time.monotonic()
processes.run(['cargo', 'build', '--manifest-path', str(GAME/'Cargo.toml'), '--bin', 'titan-game', '--bin', 'replay'], check=True, phase="build")
build_seconds=time.monotonic()-started
processes.run(['cargo', 'build', '--manifest-path', str(REPO/'Cargo.toml'), '-p', 'titan-cli'], check=True, phase="build")
def target(manifest):
    return Path(json.loads(processes.check_output(['cargo','metadata','--no-deps','--format-version','1','--manifest-path',str(manifest)], phase='build'))['target_directory'])
CLI=target(REPO/'Cargo.toml')/'debug/titan'
BINARY=target(GAME/'Cargo.toml')/'debug/titan-game'
evidence=GAME/'target/arena-evidence'; evidence.mkdir(parents=True,exist_ok=True)
instance=f'arena-test-{os.getpid()}'
p=processes.Popen([str(BINARY),'--serve','--instance',instance,'--allow-mutation','--run-for-ms','120000'],project=GAME,instance=instance,cwd=GAME,stdout=subprocess.DEVNULL)
def call(*args, error=None):
    r=processes.run([str(CLI),'--format','json','--project',str(GAME),'--instance',instance,*map(str,args)],capture_output=True,text=True)
    data=json.loads(r.stdout)
    if error: assert data['error']['code']==error,data
    else: assert data['status']=='success',data
    return data
def wait_ready(process):
    deadline=time.monotonic()+10
    while True:
        assert process.poll() is None, 'Arena exited before registration'
        if call('instances')['instances']: return
        assert time.monotonic()<deadline, 'Arena did not register within 10 seconds'
        time.sleep(.05)
try:
    startup_started=time.monotonic()
    wait_ready(p)
    startup_seconds=time.monotonic()-startup_started
    assert call('capabilities')['response']['mutation_enabled']
    entities=call('entities')['response']['entities']; player=next(e['id'] for e in entities if e['name']=='player')
    idx,gen=player['index'],player['generation']
    detail=call('entity',idx,gen)['response']; component=next(c for c in detail['components'] if c.endswith('::Position'))
    initial=call('capture')['response']; assert initial['checksum']=='e096abf94fd12c24'; shutil.copy(initial['artifact'],evidence/'initial.ppm')
    call('input',1,'--actions','{"right":{"kind":"button","value":true}}');call('step',1)
    assert call('entity',idx,gen)['response']['components'][component]['x']==81
    call('set-field',idx,gen,component,'x','--value',20)
    assert call('entity',idx,gen)['response']['components'][component]['x']==20
    failure=call('set-field',idx,gen,component,'x','--value',-1,error='invalid_value')
    bundle=Path(failure['error']['details']['diagnostic_bundle']); data=json.loads(bundle.read_text())
    assert data['world_state']['positions']['run']['health']==3
    assert data['history']['accepted_inputs']
    call('invoke','restart'); assert call('capture')['response']['checksum']==initial['checksum']
    dash_started=time.monotonic()
    clock=call('status')['response']['current_frame']
    for tick in range(1,122):
        call('input',clock+tick,'--actions',json.dumps({'dash':{'kind':'button','value':True}}))
    call('step',1)
    assert call('entity',idx,gen)['response']['components'][component]=={'x':84,'y':65}
    active=call('capture')['response']; shutil.copy(active['artifact'],evidence/'dash-active.ppm')
    diagnostic=call('invoke','verify_survival',error='invalid_value')
    dash=json.loads(Path(diagnostic['error']['details']['diagnostic_bundle']).read_text())['world_state']['positions']['run']
    assert (dash['dash_remaining'],dash['dash_cooldown'],dash['dash_ready'])==(5,120,False),dash
    call('step',5)
    assert call('entity',idx,gen)['response']['components'][component]=={'x':104,'y':65}
    cooldown=call('capture')['response']; shutil.copy(cooldown['artifact'],evidence/'dash-cooldown.ppm')
    call('step',115)
    assert call('entity',idx,gen)['response']['components'][component]=={'x':104,'y':65},'held dash must not retrigger'
    call('input',clock+122,'--actions','{"left":{"kind":"button","value":true}}');call('step',1)
    call('input',clock+123,'--actions','{"dash":{"kind":"button","value":true}}');call('step',1)
    assert call('entity',idx,gen)['response']['components'][component]=={'x':99,'y':65},'released dash uses last direction'
    call('input',clock+124,'--actions','{"dash":{"kind":"button","value":true}}')
    call('invoke','restart');call('step',1)
    assert call('entity',idx,gen)['response']['components'][component]=={'x':80,'y':65},'restart clears pending dash'
    dash_seconds=time.monotonic()-dash_started
    call('invoke','restart')
    clock=call('status')['response']['current_frame']
    for tick in range(1200):
        t=(tick-90)%360
        action='up' if tick<30 else 'right' if tick<90 else 'down' if t<60 else 'left' if t<180 else 'up' if t<240 else 'right'
        call('input',clock+tick+1,'--actions',json.dumps({action:{'kind':'button','value':True}}))
    call('step',1200);call('invoke','verify_survival')
    won=call('capture')['response']; assert won['checksum']=='b5cf61da6f50efd7',won
    shutil.copy(won['artifact'],evidence/'won.ppm')
    (evidence/'verified.json').write_text(json.dumps({'initial_checksum':initial['checksum'],'won':won,'seed':41700,'ticks':1200,'dash_active_checksum':active['checksum'],'dash_cooldown_checksum':cooldown['checksum'],'timings_seconds':{'arena_debug_build':build_seconds,'registration_poll':startup_seconds,'dash_cli_scenario':dash_seconds}},indent=2))
    # Save/load through the standalone native controlled runtime, without a window.
    call('invoke','restart')
    save_clock=call('status')['response']['current_frame']
    call('input',save_clock+1,'--actions','{"dash":{"kind":"button","value":true}}');call('step',1)
    saved=call('query','save')['response']['value']
    saved_capture=call('capture')['response']['checksum']
    (evidence/'native-save.json').write_text(json.dumps(saved,indent=2))
    call('step',20)
    before_load=call('status')
    rejected=call('invoke','load_save','--arguments','{"save":{}}',error='invalid_value')
    assert (rejected['observed_frame'],rejected['state_revision'])==(before_load['observed_frame'],before_load['state_revision'])
    load_arguments=evidence/'native-load-arguments.json'
    load_arguments.write_text(json.dumps({'save':saved}))
    malformed_arguments=evidence/'malformed-load-arguments.json'
    malformed_arguments.write_text('{')
    malformed=processes.run([str(CLI),'--format','json','--project',str(GAME),'--instance',instance,'invoke','load_save','--arguments-file',str(malformed_arguments)],capture_output=True,text=True)
    assert malformed.returncode != 0,malformed
    unchanged=call('status')
    assert (unchanged['observed_frame'],unchanged['state_revision'])==(before_load['observed_frame'],before_load['state_revision'])
    loaded=call('invoke','load_save','--arguments-file',load_arguments)
    assert loaded['observed_frame']==before_load['observed_frame']
    assert loaded['state_revision']>before_load['state_revision']
    assert call('capture')['response']['checksum']==saved_capture
    assert call('query','save')['response']['value']==saved
    assert call('query','recording')['response']['value']['invalid_reason'] is None
    # Snapshot-backed recording begins mid-dash, then reproduces all hidden state.
    for offset, action in enumerate(['left','up','up','right','down','left','up','right'], 1):
        call('input',before_load['observed_frame']+offset,'--actions',json.dumps({action:{'kind':'button','value':True}}))
    call('step',8)
    snapshot_recording=call('query','recording')
    snapshot_path=evidence/'native-snapshot-recording.json'
    snapshot_path.write_text(json.dumps(snapshot_recording))
    verification=json.loads(processes.check_output([str(BINARY.with_name('replay')),str(snapshot_path)],text=True))
    assert verification['save']==call('query','save')['response']['value']
    assert verification['checksum']==call('capture')['response']['checksum']
    assert snapshot_recording['response']['value']['initial_snapshot']==saved
    legacy=json.loads(processes.check_output([str(BINARY.with_name('replay')),str(GAME/'tests/fixtures/recording-v1.json')],text=True))
    assert legacy['ticks']==194 and legacy['checksum']=='ae923e36040921f9'

    call('invoke','restart');call('step',310)
    lost=call('invoke','verify_survival',error='invalid_value')
    data=json.loads(Path(lost['error']['details']['diagnostic_bundle']).read_text())
    assert data['world_state']['positions']['run']['outcome']=='Lost'
    shutil.copy(call('capture')['response']['artifact'],evidence/'lost.ppm')
finally:
    try:
        processes.graceful_shutdown(p)
        assert not call('instances')['instances']
    finally:
        processes.terminate(p)
p=processes.Popen([str(BINARY),'--serve','--instance',instance,'--run-for-ms','10000'],project=GAME,instance=instance,cwd=GAME,stdout=subprocess.DEVNULL)
try:
    wait_ready(p)
    call('set-field',idx,gen,component,'x','--value',20,error='mutation_disabled')
finally:
    processes.terminate(p)
print('Arena native discovery, fields, input, dash/cooldown/held/rearm, survival, loss, restart, save/load, snapshot/v1 replay, capture and bounded diagnostics passed.')
