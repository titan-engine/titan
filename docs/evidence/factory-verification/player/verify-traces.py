#!/usr/bin/env python3
"""Replay independent browser repairs through the native sequence host."""
import json
import subprocess
import tempfile
from pathlib import Path
ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent
binary = ROOT / 'games/factory/target/debug/titan-factory'
operations = []
checkpoints = []
def op(**value):
    operations.append(value)
def place(x, kind='conveyor'):
    op(op='place', kind=kind, x=x, y=3, facing='E')
def advance(ticks):
    for _ in range(ticks):
        op(op='advance', ticks=1)
def mark(name):
    checkpoints.append((len(operations)-1, name))
place(1, 'extractor')
for x in range(2,10): place(x)
advance(600); mark('ore-at-delivery')
op(op='remove',x=5,y=3); place(5,'processor')
for x in range(6,10):
    op(op='remove',x=x,y=3); place(x)
advance(600); mark('processor-repair')
op(op='remove',x=5,y=3); mark('processor-removed')
place(5,'processor'); advance(726); mark('repaired-complete')
op(op='restart'); mark('reset')
place(1,'extractor')
for x in [2,3,4,6,7,8,9]: place(x)
place(5,'processor');op(op='rotate',x=5,y=3)
advance(600);mark('wrong-facing')
for _ in range(3): op(op='rotate',x=5,y=3)
advance(1206);mark('facing-repaired-complete')
def semantic(s):
    return {k:v for k,v in s.items() if k not in ['frame','hover','preview','inspected','selection']}
expected=json.loads((HERE/'browser-exercise.json').read_text())['snapshots']
with tempfile.NamedTemporaryFile(mode='w',suffix='.json') as f:
    json.dump(operations,f);f.flush()
    run=subprocess.run([str(binary),'--sequence',f.name],cwd=ROOT,capture_output=True,text=True,timeout=60,check=True)
result=json.loads(run.stdout)
for row in result['outcomes']:
    assert 'error' not in row,row
    s=row['state']; resident=sum(v in ('ore','plate') for t in s['structures'] for v in t['slots'].values())
    assert s['extracted']==resident+s['delivered']+s['discarded_ore']+s['discarded_plate']
for (index,name),browser in zip(checkpoints,expected):
    assert name==browser['name']
    assert semantic(result['outcomes'][index]['state'])==semantic(browser['state']),name
summary={'revision':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'operation_boundaries':len(operations),'checkpoints':[n for _,n in checkpoints],'all_native_browser_semantic_states_equal':True,'conservation_each_boundary':True,'excluded_host_ui_fields':['frame','hover','preview','inspected','selection']}
(HERE/'native-browser-traces.json').write_text(json.dumps(summary,indent=2)+'\n')
(HERE/'repair-sequence.json').write_text(json.dumps(operations,separators=(',',':'))+'\n')
print(json.dumps(summary,indent=2))
