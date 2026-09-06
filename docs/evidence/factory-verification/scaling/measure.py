#!/usr/bin/env python3
"""Reproduce #93 bounded finished-slice measurements from an exact git archive."""
import datetime, io, json, os, platform, subprocess, sys, tarfile, tempfile, time
from pathlib import Path
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
sys.path.insert(0, str(REPO / 'scripts'))
import acceptance_process as processes
REVISION = 'e4800939606889669e8a9b04650cda4bce6df37d'
def place(x, y, facing, kind='conveyor'):
    return dict(op='place', x=x, y=y, facing=facing, kind=kind)
def fixtures():
    reference = [place(x, 3, 'E', 'extractor' if x == 1 else 'processor' if x == 5 else 'conveyor') for x in range(1,10)]
    path = [(1,3), (2,3)] + [(x,4) for x in range(2,12)] + [(x,5) for x in range(11,-1,-1)] + [(x,6) for x in range(12)] + [(x,7) for x in range(11,-1,-1)]
    direction = {(1,0):'E',(-1,0):'W',(0,1):'S',(0,-1):'N'}
    long = []
    for i, (x,y) in enumerate(path):
        next_xy = path[i+1] if i+1 < len(path) else (-1,7)
        facing = direction[(next_xy[0]-x,next_xy[1]-y)]
        long.append(place(x,y,facing,'extractor' if i == 0 else 'processor' if (x,y) == (5,4) else 'conveyor'))
    occupied = set(path) | {(10,3)}
    dense = long + [place(x,y,'N','processor') for y in range(8) for x in range(12) if (x,y) not in occupied]
    return [dict(name=name, operations=ops+[dict(op='advance',ticks=warmup)]) for name,ops,warmup in [
        ('reference_active',reference,600),('long_active',long,600),('dense_active',dense,600),('dense_stalled',dense,12000)]]
def counts(state):
    structures = state['structures']; by_tile = {(b['x'],b['y']):b for b in structures}
    delta = {'N':(0,-1),'E':(1,0),'S':(0,1),'W':(-1,0)}
    opposite = {'N':'S','E':'W','S':'N','W':'E'}
    connections = 0
    for b in structures:
        if b['output']:
            dx,dy = delta[b['output']]; dest = by_tile.get((b['x']+dx,b['y']+dy))
            connections += bool(dest and opposite[b['output']] in dest['inputs'])
    return dict(structures=len(structures), geometrically_compatible_directed_connections=connections,
        kinds={k:sum(b['kind']==k for b in structures) for k in ['conveyor','extractor','processor','delivery']},
        machine_statuses={status:sum(b.get('machine_status')==status for b in structures) for status in sorted({b.get('machine_status') for b in structures}, key=str)},
        resident_items=sum(v is not None for b in structures for v in b['slots'].values()),
        output_reasons={str(reason):sum(b.get('last_transfer_reason')==reason for b in structures) for reason in sorted({b.get('last_transfer_reason') for b in structures}, key=str)},
        tick=state['tick'], extracted=state['extracted'], delivered=state['delivered'])
def main():
    report = dict(revision=REVISION, utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        environment=dict(os=platform.platform(),cpu=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip() if platform.system()=='Darwin' else platform.processor(),logical_cpus=os.cpu_count(),python=platform.python_version(),rustc=subprocess.check_output(['rustc','--version'],text=True).strip(),cargo=subprocess.check_output(['cargo','--version'],text=True).strip(),profile='Cargo dev, unoptimized + debuginfo',backend='native public game API; software capture',cache='empty private build target; existing global Cargo registry/toolchain retained',concurrency='Host CPU and global Cargo registry shared with other factory/adventure agents; private target; no GUI'),failures=[])
    if '--resume' in sys.argv:
        previous=json.loads((HERE/'results.json').read_text())
        assert previous['revision'] == REVISION
        report['previous_attempt']=dict(utc=previous['utc'], build_seconds=previous['build_seconds'], failures=previous['failures'])
        report['fixtures']=previous['fixtures']
        for data in report['fixtures']:
            for sample in data['samples']:
                if 'initial' in sample:
                    sample['initial_workload']=counts(sample['initial']); sample['final_workload']=counts(sample['final'])
            for sample in data['samples'][1:]:
                if 'initial' in sample:
                    assert sample['initial'] == data['samples'][0]['initial'] and sample['final'] == data['samples'][0]['final']
                    del sample['initial']; del sample['final']
                    sample['snapshot_reference']='repeat 0 (exact initial and final equality checked)'
    with tempfile.TemporaryDirectory(prefix='titan-factory-scaling-') as temp:
        root=Path(temp)
        with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['git','archive',REVISION],cwd=REPO))) as archive:
            archive.extractall(root)
        game=root/'games/factory'; (game/'src/bin/scaling_probe.rs').write_bytes((HERE/'probe.rs').read_bytes())
        env=dict(os.environ,CARGO_TARGET_DIR=str(root/'build'))
        start=time.perf_counter()
        built=processes.run(['cargo','build','--locked','--manifest-path',str(game/'Cargo.toml'),'--bin','scaling_probe'],cwd=root,env=env,phase='build',capture_output=True,text=True)
        report['build_seconds']=time.perf_counter()-start
        if built.returncode: raise RuntimeError(built.stderr.replace(str(root),'<scratch>'))
        report['runtime_timeout_seconds']=300
        report.setdefault('fixtures', [])
        for fixture in fixtures():
            if any(f['name'] == fixture['name'] for f in report['fixtures']):
                assert json.loads((HERE/(fixture['name']+'.json')).read_text()) == fixture
                continue
            path=HERE/(fixture['name']+'.json'); path.write_text(json.dumps(fixture,indent=2)+'\n')
            print('Measuring '+fixture['name'],flush=True)
            start=time.perf_counter()
            try:
                result=processes.run([str(root/'build/debug/scaling_probe'),str(path)],cwd=root,env=env,capture_output=True,text=True,timeout=300)
                if result.returncode: raise RuntimeError(result.stderr.replace(str(root),'<scratch>'))
            except Exception as error:
                report['failures'].append(dict(fixture=fixture['name'], error=str(error).replace(str(root),'<scratch>')))
                (HERE/'results.json').write_text(json.dumps(report,indent=2)+'\n')
                raise
            data=json.loads(result.stdout); data['subprocess_seconds']=time.perf_counter()-start
            for sample in data['samples']:
                sample['initial_workload']=counts(sample['initial']);sample['final_workload']=counts(sample['final'])
            for sample in data['samples'][1:]:
                assert sample['initial'] == data['samples'][0]['initial'] and sample['final'] == data['samples'][0]['final']
                del sample['initial']; del sample['final']
                sample['snapshot_reference']='repeat 0 (exact initial and final equality checked)'
            report['fixtures'].append(data)
            (HERE/'results.json').write_text(json.dumps(report,indent=2)+'\n')
        report['cleanup']='All probes exited normally; no server/discovery registration/window created; disposable archive and target removed on context exit.'
    (HERE/'results.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Wrote results.json')
if __name__ == '__main__': main()
