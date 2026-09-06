// Independent expectations from docs/factory-slice.md. Both hosts execute the same
// player operations; fixture seeding is explicitly counted and never an operation.
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFile} from '../../../scripts/acceptance_process.mjs';

export async function productionAcceptance({BrowserRuntime,root,target,raw,state}) {
  const directory=mkdtempSync(join(tmpdir(),'factory-production-'));
  const at=(s,x,y=3)=>s.structures.find(t=>t.x===x&&t.y===y);
  const output=(s,x,y=3)=>at(s,x,y)?.slots.output ?? null;
  const ticks=n=>Array.from({length:n},()=>({op:'advance',ticks:1}));
  const place=(kind,x,y=3,facing='E')=>({op:'place',kind,x,y,facing});
  const route=JSON.parse(readFileSync(join(root,'tests/production-route.json'),'utf8'));
  // The game frame is host time; completion freezes every other inspected field.
  const simulation=({frame,...rest})=>rest;
  const machine=s=>{const {facing,slots,progress,remaining}=at(s,5);return {facing,slots,progress,remaining};};
  function accounting(s,label) {
    let ore=0,plate=0;
    for(const t of s.structures) {
      for(const item of Object.values(t.slots)) {
        assert.ok(item===null || item==='ore' || item==='plate',label);
        ore+=Number(item==='ore');plate+=Number(item==='plate');
      }
      assert.equal(t.item_positions.length,Object.values(t.slots).filter(v=>v!==null).length,label);
    }
    assert.equal(s.seeded+s.extracted,ore+plate+s.delivered+s.discarded_ore+s.discarded_plate,`${label}: independent item accounting`);
    assert.equal(s.conserved,true,label);
  }
  const cases=[
    {name:'reference route',ops:[...route,...ticks(1269),...ticks(3),{op:'place',kind:'conveyor',x:0,y:0,facing:'E'},{op:'remove',x:1,y:3},{op:'rotate',x:5,y:3},{op:'restart'},...route,...ticks(1269)]},
    {name:'extractor backpressure',fixture:'isolated_extractor',seeded:0,ops:[...ticks(180),place('conveyor',2),...ticks(60),{op:'restart'}]},
    {name:'processor starvation',fixture:'processor_input',seeded:1,ops:[...ticks(180),{op:'restart'}]},
    {name:'processor blocked output',fixture:'processor_blocked',seeded:2,ops:[...ticks(121),place('conveyor',6),place('conveyor',7),...ticks(3),{op:'restart'}]},
    {name:'full processor removal',fixture:'processor_full',seeded:3,ops:[...ticks(10),{op:'rotate',x:5,y:3},{op:'remove',x:10,y:3},{op:'place',kind:'processor',x:5,y:3,facing:'E'},{op:'remove',x:5,y:3},place('processor',5),{op:'restart'}]},
    {name:'finished batch backlog',fixture:'processor_full',seeded:3,ops:[...ticks(10),place('conveyor',6),...ticks(1),{op:'rotate',x:5,y:3},...ticks(120),{op:'restart'}]},
    {name:'occupied extractor edits',fixture:'isolated_extractor',seeded:0,ops:[...ticks(60),{op:'rotate',x:1,y:3},{op:'remove',x:1,y:3},place('extractor',1),...ticks(59),{op:'remove',x:1,y:3},{op:'restart'}]},
    {name:'rejected player edits',ops:[...route,...ticks(64),{op:'seed',x:2,y:3,item:'ore'},{op:'remove',x:10,y:3},{op:'rotate',x:10,y:3},place('delivery',0),place('extractor',0),place('processor',1),place('conveyor',-1),place('conveyor',0,0,'bad'),{op:'restart'}]},
  ];
  try {
    assert.throws(()=>BrowserRuntime.production_fixture('not-a-fixture',true));
    const readonly=BrowserRuntime.production_fixture('processor_input',false);
    try {assert.equal(raw(readonly,{type:'step',frames:1}).error.code,'mutation_disabled');} finally {readonly.free();}
    for(const [index,c] of cases.entries()) {
      const path=join(directory,`${index}.json`);writeFileSync(path,JSON.stringify(c.ops));
      const native=JSON.parse(await execFile(join(target,'debug/titan-factory'),[...(c.fixture?['--production-fixture',c.fixture]:[]),'--sequence',path],{cwd:root}));
      const game=c.fixture?BrowserRuntime.production_fixture(c.fixture,true):new BrowserRuntime(true);
      try {
        const trace=[state(game)];
        assert.equal(trace[0].seeded,c.seeded??0);
        for(const [i,operation] of c.ops.entries()) {
          const before=state(game);
          const response=raw(game,{type:'invoke',name:'construct',arguments:operation});
          assert.equal(response.status,native.outcomes[i].error?'failure':'success',`${c.name} result ${i}`);
          const current=state(game);trace.push(current);
          assert.deepEqual(current,native.outcomes[i].state,`${c.name} native/compiled WASM boundary ${i}`);
          accounting(current,`${c.name} ${i}`);
          if(response.status==='failure')assert.deepEqual(current,before,'rejected operation is atomic');
          if(operation.op==='restart') {
            assert.equal(current.tick,0);assert.equal(current.seeded,0);assert.equal(current.extracted,0);
            assert.equal(current.delivered,0);assert.equal(current.discarded_ore,0);assert.equal(current.discarded_plate,0);
            assert.equal(current.completion_tick,null);assert.equal(current.outcome,'Running');
            assert.equal(current.structures.length,1);assert.equal(current.selection.kind,'conveyor');
          }
        }
        if(index===0) {
          const run=trace.slice(route.length,route.length+1270);
          for(const s of run) {
            assert.equal(s.seeded,0);assert.equal(s.discarded_ore+s.discarded_plate,0);
            assert.equal(s.delivered,s.tick<189?0:Math.min(10,1+Math.floor((s.tick-189)/120)));
          }
          assert.equal(at(run[59],1).progress,59);assert.equal(output(run[59],1),null);
          assert.equal(run[60].extracted,1);assert.equal(output(run[60],1),'ore');assert.equal(at(run[60],1).progress,0);
          for(const [tick,x] of [[61,2],[62,3],[63,4]])assert.equal(output(run[tick],x),'ore');
          assert.equal(at(run[64],5).slots.in_process,'ore');assert.equal(at(run[64],5).remaining,120);
          assert.equal(run[120].extracted,2);assert.equal(output(run[120],1),'ore');assert.equal(at(run[124],5).slots.input,'ore');
          assert.equal(at(run[183],5).remaining,1);assert.equal(output(run[183],5),null);
          assert.equal(output(run[184],5),'plate');assert.equal(at(run[184],5).remaining,120);
          assert.equal(at(run[184],5).slots.input,null);assert.equal(at(run[184],5).slots.in_process,'ore');
          for(const [tick,x] of [[185,6],[186,7],[187,8],[188,9]])assert.equal(output(run[tick],x),'plate');
          const complete=run[1269];assert.equal(complete.outcome,'Complete');assert.equal(complete.completion_tick,1269);
          // At completion the ten delivered plates plus five upstream ores account
          // for all fifteen extractions: one batch, one queued input, three belts.
          assert.equal(complete.extracted,15);
          assert.equal(at(complete,5).slots.in_process,'ore');assert.equal(at(complete,5).slots.input,'ore');
          for(const x of [2,3,4])assert.equal(output(complete,x),'ore');
          assert.equal(output(complete,1),null);assert.equal(output(complete,5),null);
          assert.equal(at(complete,5).remaining,116);assert.equal(at(complete,1).progress,1);
          complete.structures.forEach(t=>assert.equal(t.machine_status,'complete'));
          // Completion skips production, including work already in flight.
          assert.equal(at(complete,5).remaining,at(run[1268],5).remaining);
          assert.equal(at(complete,1).progress,at(run[1268],1).progress);
          for(const s of trace.slice(route.length+1270,route.length+1276))assert.deepEqual(simulation(s),simulation(complete));
          assert.deepEqual(simulation(trace.at(-1)),simulation(complete),'restart reproduces the exact completed challenge');
        } else if(index===1) {
          assert.equal(at(trace[59],1).progress,59);assert.equal(output(trace[60],1),'ore');
          for(const s of trace.slice(60,181)){assert.equal(at(s,1).progress,0);assert.equal(s.extracted,1);}
          assert.equal(at(trace[182],1).progress,1);assert.equal(output(trace[182],2),'ore');
          assert.equal(at(trace[240],1).progress,59);assert.equal(output(trace[241],1),'ore');assert.equal(trace[241].extracted,2);
        } else if(index===2) {
          assert.equal(at(trace[0],5).machine_status,'waiting_for_ore');assert.equal(at(trace[1],5).machine_status,'processing');assert.equal(at(trace[1],5).remaining,120);assert.equal(at(trace[120],5).remaining,1);
          assert.equal(output(trace[121],5),'plate');assert.equal(at(trace[121],5).slots.in_process,null);
          for(const s of trace.slice(121,181)){assert.equal(output(s,5),'plate');assert.equal(at(s,5).slots.in_process,null);assert.equal(s.extracted,0);}
        } else if(index===3) {
          assert.equal(at(trace[120],5).remaining,1);assert.equal(at(trace[121],5).remaining,0);
          assert.equal(at(trace[121],5).machine_status,'finished_batch_blocked');assert.equal(at(trace[121],5).slots.in_process,'ore');assert.equal(output(trace[121],5),'plate');
          const released=trace[124];assert.equal(released.tick,122);assert.equal(output(released,6),'plate');
          assert.equal(output(released,5),'plate');assert.equal(at(released,5).slots.in_process,null);
          assert.equal(output(trace[125],5),'plate','snapshot-full next belt prevents same-tick chain movement');
          assert.equal(output(trace[126],5),null);assert.equal(output(trace[126],6),'plate');
        } else if(index===4) {
          assert.equal(at(trace[10],5).remaining,0);assert.equal(at(trace[10],5).slots.input,'ore');
          assert.deepEqual({...machine(trace[11]),facing:'E'},machine(trace[10]),'rotation preserves all machine contents and work');
          assert.equal(trace[14].discarded_ore,2);assert.equal(trace[14].discarded_plate,1);assert.equal(trace[14].delivered,0);
          assert.equal(at(trace[15],5).slots.in_process,null);assert.equal(at(trace[15],5).remaining,0);
        } else if(index===5) {
          const released=trace[12];assert.equal(released.tick,11);assert.equal(at(released,5).remaining,120);
          assert.equal(at(released,5).slots.input,null);assert.equal(at(released,5).slots.in_process,'ore');assert.equal(output(released,5),'plate');
          assert.deepEqual({...machine(trace[13]),facing:'E'},machine(released));
          assert.equal(at(trace[133],5).remaining,0);assert.equal(at(trace[133],5).slots.in_process,'ore');assert.equal(output(trace[133],5),'plate');
        } else if(index===6) {
          assert.equal(output(trace[61],1),'ore');assert.equal(at(trace[61],1).progress,0);
          assert.equal(trace[62].discarded_ore,1);assert.equal(trace[62].discarded_plate,0);
          assert.equal(at(trace[122],1).progress,59);assert.equal(trace[123].discarded_ore,1,'partial progress is not a discarded ore');
        }
        console.log(`Production ${c.name}: ${c.ops.length} native/compiled-WASM boundaries, independent timing and accounting passed.`);
      } finally {game.free();}
    }
  } finally {rmSync(directory,{recursive:true,force:true});}
}
