#!/usr/bin/env python3
"""Bounded standalone native CLI acceptance; run from any directory."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

GAME = Path(__file__).resolve().parents[1]
REPO = GAME.parents[1]
subprocess.run(['cargo', 'build', '--manifest-path', str(GAME/'Cargo.toml'), '--bin', 'titan-game'], check=True)
subprocess.run(['cargo', 'build', '--manifest-path', str(REPO/'Cargo.toml'), '-p', 'titan-cli'], check=True)
def target(manifest):
    return Path(json.loads(subprocess.check_output(['cargo','metadata','--no-deps','--format-version','1','--manifest-path',str(manifest)]))['target_directory'])
CLI=target(REPO/'Cargo.toml')/'debug/titan'
BINARY=target(GAME/'Cargo.toml')/'debug/titan-game'
evidence=GAME/'target/arena-evidence'; evidence.mkdir(parents=True,exist_ok=True)
instance=f'arena-test-{os.getpid()}'
p=subprocess.Popen([str(BINARY),'--serve','--instance',instance,'--allow-mutation','--run-for-ms','120000'],cwd=GAME,stdout=subprocess.DEVNULL)
def call(*args, error=None):
    r=subprocess.run([str(CLI),'--format','json','--project',str(GAME),'--instance',instance,*map(str,args)],capture_output=True,text=True,timeout=10)
    data=json.loads(r.stdout)
    if error: assert data['error']['code']==error,data
    else: assert data['status']=='success',data
    return data
try:
    for _ in range(100):
        time.sleep(.05)
        r=subprocess.run([str(CLI),'--format','json','--project',str(GAME),'--instance',instance,'instances'],capture_output=True,text=True)
        if json.loads(r.stdout).get('instances'): break
    assert call('capabilities')['response']['mutation_enabled']
    entities=call('entities')['response']['entities']; player=next(e['id'] for e in entities if e['name']=='player')
    idx,gen=player['index'],player['generation']
    detail=call('entity',idx,gen)['response']; component=next(c for c in detail['components'] if c.endswith('::Position'))
    initial=call('capture')['response']; assert initial['checksum']=='1e5d05f547d53435'; shutil.copy(initial['artifact'],evidence/'initial.ppm')
    call('input',1,'--actions','{"right":{"kind":"button","value":true}}');call('step',1)
    assert call('entity',idx,gen)['response']['components'][component]['x']==81
    call('set-field',idx,gen,component,'x','--value',20)
    assert call('entity',idx,gen)['response']['components'][component]['x']==20
    failure=call('set-field',idx,gen,component,'x','--value',-1,error='invalid_value')
    bundle=Path(failure['error']['details']['diagnostic_bundle']); data=json.loads(bundle.read_text())
    assert data['world_state']['positions']['run']['health']==3
    assert data['history']['accepted_inputs']
    call('invoke','restart'); assert call('capture')['response']['checksum']==initial['checksum']
    clock=call('status')['response']['current_frame']
    for tick in range(1200):
        t=(tick-90)%360
        action='up' if tick<30 else 'right' if tick<90 else 'down' if t<60 else 'left' if t<180 else 'up' if t<240 else 'right'
        call('input',clock+tick+1,'--actions',json.dumps({action:{'kind':'button','value':True}}))
    call('step',1200);call('invoke','verify_survival')
    won=call('capture')['response']; assert won['checksum']=='be61b1c710b101b6',won
    shutil.copy(won['artifact'],evidence/'won.ppm')
    (evidence/'verified.json').write_text(json.dumps({'initial_checksum':initial['checksum'],'won':won,'seed':41700,'ticks':1200},indent=2))
    call('invoke','restart');call('step',310)
    lost=call('invoke','verify_survival',error='invalid_value')
    data=json.loads(Path(lost['error']['details']['diagnostic_bundle']).read_text())
    assert data['world_state']['positions']['run']['outcome']=='Lost'
    shutil.copy(call('capture')['response']['artifact'],evidence/'lost.ppm')
finally:
    p.terminate();p.wait(timeout=5)
assert not call('instances')['instances']
p=subprocess.Popen([str(BINARY),'--serve','--instance',instance,'--run-for-ms','10000'],cwd=GAME,stdout=subprocess.DEVNULL)
try:
    time.sleep(.2)
    call('set-field',idx,gen,component,'x','--value',20,error='mutation_disabled')
finally:
    p.terminate();p.wait(timeout=5)
print('Arena native discovery, fields, input, survival, loss, restart, capture and bounded diagnostics passed.')
