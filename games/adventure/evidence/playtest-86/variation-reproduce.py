#!/usr/bin/env python3
"""Run against a disposable checkout with variation.patch applied; no teleport.
Usage: python3 variation-reproduce.py SCRATCH_ROOT OUTPUT_DIRECTORY
Builds use an isolated target directory. All hosts/commands are bounded.
"""
import json, os, signal, subprocess, sys, time
from pathlib import Path
ROOT=Path(sys.argv[1]).resolve(); OUT=Path(sys.argv[2]).resolve(); OUT.mkdir(parents=True,exist_ok=True)
FIXTURE=Path(__file__).with_name('variation-route.json')
TARGET=ROOT/'target/variation'; GAME=ROOT/'games/adventure'; INSTANCE=f'variation-86-{os.getpid()}'
env=dict(os.environ,CARGO_TARGET_DIR=str(TARGET))
results={'phases':[], 'checkpoints':{}, 'calls':0, 'cleanup_verified':False}
def run(args,timeout=60):
    start=time.monotonic(); p=subprocess.run(list(map(str,args)),cwd=ROOT,env=env,capture_output=True,text=True,timeout=timeout)
    return p,time.monotonic()-start
for manifest,opts in [(ROOT/'Cargo.toml',['-p','titan-cli']),(GAME/'Cargo.toml',['--bin','titan-adventure'])]:
    p,elapsed=run(['cargo','build','--manifest-path',manifest,*opts],1200)
    results['phases'].append({'phase':'build '+('cli' if manifest.parent==ROOT else 'adventure'),'seconds':elapsed,'returncode':p.returncode})
    with (OUT/'variation-build.log').open('a') as f:f.write(p.stderr.replace(str(ROOT),'<scratch>'))
    p.check_returncode()
CLI=TARGET/'debug/titan'; HOST=TARGET/'debug/titan-adventure'
def call(*args,error=None):
    p,elapsed=run([CLI,'--format','json','--project',GAME,'--instance',INSTANCE,*args]); results['calls']+=1
    data=json.loads(p.stdout)
    if error: assert p.returncode != 0 and data['error']['code']==error,data
    else: assert p.returncode==0 and data['status']=='success',data
    return data
def state():return call('query','state')['response']['value']
def invoke(name,args=None,error=None):
    path=OUT/'variation-arguments.json'; path.write_text(json.dumps(args or {}))
    return call('invoke',name,'--arguments-file',path,error=error)
log=(OUT/'variation-host.log').open('w')
host=subprocess.Popen([str(HOST),'--serve','--project',str(GAME),'--instance',INSTANCE,'--run-for-ms','120000'],cwd=ROOT,env=env,stdout=log,stderr=log,start_new_session=True)
start=time.monotonic()
try:
    deadline=start+10
    while not any(x['instance_id']==INSTANCE for x in call('instances')['instances']):
        assert host.poll() is None and time.monotonic()<deadline
        time.sleep(.05)
    for name in ('capabilities','commands','queries','entities'):
        results[name]=call(name)['response']
    invoke('select_room',{'room':2})
    assert state()['puzzle_geometry']['plates'][1]['min_z']==4700
    invoke('select_room',{'room':1})
    results['initial']=state()
    assert results['initial']['puzzle_geometry']['plates'][1]['min_z']==5300
    assert results['initial']['puzzle_geometry']['plates'][1]['max_z']==5900
    frame=call('status')['response']['current_frame']
    for segment in json.loads(FIXTURE.read_text()):
        for _ in range(segment['ticks']):
            frame+=1
            call('input',frame,'--actions',json.dumps({a:{'kind':'button','value':True} for a in segment['actions']}))
        call('step',segment['ticks'])
        if 'checkpoint' in segment:
            s=state(); key=segment['checkpoint']; results['checkpoints'][key]=s
            if key=='plate-a':assert s['puzzle']['plates'][0]['occupants']==['jumper'],s
            if key=='plate-b':assert s['puzzle']['plates'][1]['occupants']==['strong'],s
            if key=='exchange':assert s['puzzle']['door']['open'] and s['puzzle']['plates'][1]['pressed'],s
            if key=='jumper-exit':assert s['puzzle']['exit']['jumper'],s
            if key=='complete':assert s['puzzle']['complete'] and s['phase']=='room_complete',s
    expected=state(); recording=call('query','recording')['response']['value']
    (OUT/'variation-recording.json').write_text(json.dumps(recording,indent=2)+'\n')
    before=state(); bad=invoke('replay',{'recording':{**recording,'fixture':'deliberately-invalid'}},error='invalid_value')
    assert state()==before
    diagnostic=bad['error'].get('details',{}).get('diagnostic_bundle')
    if diagnostic:
        manifest=json.loads(Path(diagnostic).read_text())
        api=Path(diagnostic).with_name('api.txt').read_text()
        assert 'replay' in api and manifest['response']['error']['code']=='invalid_value'
        results['diagnostic_summary']={'bundle_version':manifest['bundle_version'],'response':manifest['response'],'capture':manifest['capture'],'api_summary':manifest['api_summary'],'world_state_present':bool(manifest['world_state']),'api_text_read':True}
        (OUT/'variation-diagnostic-api.txt').write_text(api)
    results['deliberate_rejection']=bad
    invoke('restart'); assert not state()['puzzle']['complete']
    invoke('replay',{'recording':recording}); actual=state()
    keys=('characters','active_character','session_tick','consumed_input','puzzle','room','phase','block')
    for key in keys:assert actual.get(key)==expected.get(key),(key,actual,expected)
    results['replay_compared_keys']=keys; results['replay_equal']=True
    results['phases'].append({'phase':'runtime','seconds':time.monotonic()-start,'returncode':0})
finally:
    os.killpg(host.pid,signal.SIGTERM)
    try:host.wait(timeout=10)
    except subprocess.TimeoutExpired:os.killpg(host.pid,signal.SIGKILL);host.wait(timeout=5)
    log.close()
    host_log=OUT/'variation-host.log'
    host_log.write_text(host_log.read_text().replace(str(ROOT),'<scratch>'))
    results['cleanup_verified']=not any(x['instance_id']==INSTANCE for x in call('instances')['instances'])
    text=json.dumps(results,indent=2).replace(str(ROOT),'<scratch>').replace(str(OUT),'<evidence>')
    (OUT/'variation-results.json').write_text(text+'\n')
    (OUT/'variation-arguments.json').unlink(missing_ok=True)
assert results['cleanup_verified']
print(json.dumps({'passed':True,'calls':results['calls'],'phases':results['phases'],'cleanup':results['cleanup_verified']}))
