// Independent expected repair traces for the read-only explanation contract.
import assert from 'node:assert/strict';
export function interfaceAcceptance({BrowserRuntime,ok,state}) {
  const cases=[
    {fixture:'disconnected',x:6,y:5,code:'disconnected',repair:[{op:'place',kind:'conveyor',x:7,y:5,facing:'E'}]},
    {fixture:'ports',x:2,y:0,code:'wrong_facing',repair:[{op:'rotate',x:3,y:0}]},
    {fixture:'ports',x:5,y:0,code:'wrong_type',repair:[{op:'remove',x:6,y:0},{op:'place',kind:'conveyor',x:6,y:0,facing:'E'}]},
    {fixture:'cycle_full',x:2,y:2,code:'full',repair:[{op:'remove',x:3,y:2},{op:'place',kind:'conveyor',x:3,y:2,facing:'S'}]},
    {fixture:'contention',x:2,y:3,code:'contended',repair:[{op:'rotate',x:3,y:2}]},
  ];
  for(const {fixture,x,y,code,repair} of cases) {
    const runtime=BrowserRuntime.transport_fixture(fixture,true);
    try {
      const before=state(runtime), metadata=ok(runtime,{type:'status'});
      const recording=ok(runtime,{type:'query',name:'recording',arguments:{}});
      const tile=ok(runtime,{type:'query',name:'tile',arguments:{x,y}}).value.structure;
      const ui=ok(runtime,{type:'query',name:'interface',arguments:{}}).value;
      assert.deepEqual(ui.structures.find(s=>s.x===x&&s.y===y),tile);
      assert.equal(tile.connection.code,code);
      assert.equal(tile.explanation.status,'output_blocked');
      for(const key of ['label','detail','remedy']) assert.ok(tile.connection[key].length>8);
      ok(runtime,{type:'query',name:'preview',arguments:{x,y,action:'rotate'}});
      ok(runtime,{type:'capture'});
      assert.deepEqual(state(runtime),before,'queries and rendering are immutable');
      assert.deepEqual(ok(runtime,{type:'status'}),metadata);
      assert.deepEqual(ok(runtime,{type:'query',name:'recording',arguments:{}}),recording);
      for(const operation of repair) ok(runtime,{type:'invoke',name:'construct',arguments:operation});
      const repaired=ok(runtime,{type:'query',name:'tile',arguments:{x,y}}).value.structure;
      assert.equal(repaired.connection.code,'ready',`${code} repair must remove its actual cause`);
      assert.equal(state(runtime).tick,before.tick,'repairs happen while paused');
      ok(runtime,{type:'step',frames:1});
      assert.equal(state(runtime).structures.find(s=>s.x===x&&s.y===y).slots.output,null,'repaired item must transfer');
      assert.equal(state(runtime).conserved,true);
    } finally {runtime.free();}
  }
  console.log('Factory immutable UI/query explanations and five deliberate bottleneck repairs passed in actual WASM.');
}
