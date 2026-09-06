// Independent design traces, exercised through both the native CLI and actual WASM.
import assert from 'node:assert/strict';
import {mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFile} from '../../../scripts/acceptance_process.mjs';

export async function transportAcceptance({BrowserRuntime,root,target,raw,ok,state}) {
  await execFile('cargo',['build', '--locked','--bin','titan-factory'],{phase:'build',cwd:root});
  const directory=mkdtempSync(join(tmpdir(),'factory-transport-'));
  const at=(s,x,y)=>s.structures.find(t=>t.x===x&&t.y===y);
  const item=(s,x,y)=>at(s,x,y)?.slots.output ?? null;
  const step={op:'advance',ticks:1};
  const ticks=count=>Array.from({length:count},()=>step);
  const cases=[
    ['single',ticks(4)],['snapshot',ticks(5)],['contention',ticks(5)],
    ['cycle_partial',ticks(24)],['cycle_full',ticks(8)],
    ['disconnected',ticks(8)],['ports',ticks(5)],
    ['cycle_full', [...ticks(3),{op:'remove',x:2,y:2},{op:'place',kind:'conveyor',x:2,y:2,facing:'E'},...ticks(12),{op:'rotate',x:3,y:2},...ticks(4),{op:'place',kind:'conveyor',x:3,y:2,facing:'N'},{op:'remove',x:10,y:3},{op:'restart'},...ticks(2)]],
  ];
  try {
    assert.throws(()=>BrowserRuntime.transport_fixture('not-a-fixture',true));
    const readonly=BrowserRuntime.transport_fixture('single',false);
    assert.equal(raw(readonly,{type:'step',frames:1}).error.code,'mutation_disabled');readonly.free();
    for(const [index,[fixture,operations]] of cases.entries()) {
      const path=join(directory,`${index}.json`);writeFileSync(path,JSON.stringify(operations));
      const native=JSON.parse(await execFile(join(target,'debug','titan-factory'),['--transport-fixture',fixture,'--sequence',path],{cwd:root}));
      const game=BrowserRuntime.transport_fixture(fixture,true);
      try {
        const initial=state(game); const trace=[initial];
        for(const [i,operation] of operations.entries()) {
          const result=raw(game,{type:'invoke',name:'construct',arguments:operation});
          assert.equal(result.status,native.outcomes[i].error?'failure':'success',`${fixture} outcome ${i}`);
          const current=state(game);trace.push(current);
          assert.deepEqual(current,native.outcomes[i].state,`${fixture} full native/WASM state at operation ${i}`);
          assert.equal(current.conserved,true);
          const live=current.structures.reduce((sum,t)=>sum+Object.values(t.slots).filter(v=>v!==null).length,0);
          assert.equal(current.seeded+current.extracted,live+current.delivered+current.discarded_ore+current.discarded_plate,`${fixture} independent conservation`);
          for(const structure of current.structures) {
            assert.equal(structure.item_positions.length,Object.values(structure.slots).filter(v=>v!==null).length,'every item has an inspection position');
          }
        }
        assert.deepEqual(state(game),native.state);
        if(fixture==='single') {
          assert.deepEqual(trace.slice(0,4).map(s=>[item(s,2,2),item(s,3,2),item(s,4,2)]),[['ore',null,null],[null,'ore',null],[null,null,'ore'],[null,null,'ore']]);
        }
        if(fixture==='snapshot') {
          assert.deepEqual(trace.slice(0,4).map(s=>[item(s,2,2),item(s,3,2),item(s,4,2)]),[['ore','ore',null],['ore',null,'ore'],[null,'ore','ore'],[null,'ore','ore']]);
          assert.equal(at(trace[1],2,2).last_transfer_reason,'full_destination');
        }
        if(fixture==='contention') {
          assert.equal(item(trace[1],3,2),null);assert.equal(item(trace[1],2,3),'plate');
          assert.equal(at(trace[1],2,3).last_transfer_reason,'contention');
          assert.equal(at(trace[2],2,3).last_transfer_reason,'full_destination');
          assert.equal(item(trace[3],2,3),null);
        }
        if(fixture==='disconnected') {
          trace.forEach(s=>{assert.equal(item(s,0,0),'ore');assert.equal(item(s,6,5),'plate');assert.equal(at(s,0,0).transport.reason,'missing_neighbor');assert.equal(at(s,6,5).transport.reason,'missing_neighbor');});
        }
        if(fixture==='ports') {
          assert.equal(at(trace[1],2,0).last_transfer_reason,'mismatched_input_face');
          assert.equal(at(trace[1],5,0).last_transfer_reason,'rejected_item_type');
          assert.equal(item(trace[1],1,3),null);assert.equal(at(trace[1],2,3).slots.input,'ore');
          assert.equal(trace[1].delivered,1);assert.equal(item(trace[1],9,3),null);
          trace.slice(1).forEach(s=>{assert.equal(at(s,2,3).slots.input,'ore');assert.equal(at(s,2,3).slots.in_process,null);assert.equal(s.extracted,0);});
        }
        if(fixture==='cycle_partial') {
          const positions=[[2,2],[3,2],[3,3],[2,3]];
          trace.forEach((s,i)=>{const occupied=s.structures.filter(t=>t.slots.output!==null);assert.equal(occupied.length,1);assert.deepEqual([occupied[0].x,occupied[0].y],positions[i%4]);});
        }
        if(fixture==='cycle_full' && index===4) {
          trace.forEach(s=>{for(const [x,y] of [[2,2],[3,2],[3,3],[2,3]])assert.equal(item(s,x,y),'ore');});
        }
        if(index===7) {
          assert.equal(trace[4].discarded_ore,1);assert.equal(trace[5].discarded_ore,1);
          assert.equal(item(trace[6],2,2),'ore');assert.equal(item(trace[6],2,3),null);
          const reset=trace.at(-1);assert.equal(reset.seeded,0);assert.equal(reset.discarded_ore,0);assert.equal(reset.structures.length,1);
        }
        console.log(`Transport ${fixture}: ${operations.length} full native/actual-WASM boundaries, conservation and design traces passed.`);
      } finally {game.free();}
    }
  } finally {rmSync(directory,{recursive:true,force:true});}
}
