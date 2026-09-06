import test from 'node:test';
import assert from 'node:assert/strict';
import {shortcut,describeResult,renderDetails} from './interface.mjs';
test('construction shortcuts ignore repeat and platform command modifiers',()=>{
  assert.deepEqual(shortcut({key:'2'}),{kind:'extractor'});
  for(const key of ['1','q','e','x','r',' ','.']) {
    assert.equal(shortcut({key,repeat:true}),null);
    assert.equal(shortcut({key,metaKey:true}),null);
    assert.equal(shortcut({key,ctrlKey:true}),null);
  }
  assert.equal(shortcut({key:'.'}),'step');assert.equal(shortcut({key:' '}),'pause');
});
test('removal feedback exposes the actual discarded inventory delta',()=>{
  assert.equal(describeResult({}, {discarded_ore:3,discarded_plate:1},{discarded_ore:5,discarded_plate:2}), 'Removed structure. Discarded 2 ore and 1 plates.');
});
test('details preserve authoritative explanation and bounded inventory without recomputing causes',()=>{
  const nodes=[];const container={replaceChildren(){nodes.length=0;},append(el){nodes.push(el);},ownerDocument:{createElement:()=>({})}};
  const tile={kind:'processor',x:5,y:3,facing:'E',explanation:{status:'starved',label:'Waiting for ore',detail:'No input ore.',remedy:'Connect an ore source.'},inventory:[{slot:'input',item:null,count:0,capacity:1}],recipe:{label:'1 ore → 1 plate',elapsed:40,total:120},connection:{label:'Wrong type',detail:'Plate cannot enter processor.',remedy:'Route plates to delivery.'}};
  const before=JSON.stringify(tile);renderDetails(container,tile);assert.equal(JSON.stringify(tile),before);
  for(const expected of ['Waiting for ore','Connect an ore source.','input: empty (0/1)','40 / 120 work ticks','Route plates to delivery.'])assert.ok(nodes.some(n=>n.textContent===expected),expected);
  assert.ok(nodes.some(n=>n.max===120&&n.value===40));
});
