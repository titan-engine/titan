import assert from 'node:assert/strict';
import { execFile } from '../../../scripts/acceptance_process.mjs';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const metadata = JSON.parse(await execFile('cargo', ['metadata', '--format-version', '1', '--no-deps'], { phase: 'build', cwd: root, encoding: 'utf8' }));
const { BrowserRuntime } = createRequire(import.meta.url)(resolve(metadata.target_directory, 'titan/browser-node/titan_game.js'));
let sequence = 0;
const raw = (runtime, request) => JSON.parse(runtime.handle(JSON.stringify({schema_version:2,request_id:`test-${++sequence}`,request})));
function ok(runtime, request) { const response = raw(runtime, request); assert.equal(response.status, 'success', JSON.stringify(response)); return response.response; }
const invoke = (game,name,args={}) => ok(game,{type:'invoke',name,arguments:args});
const state = game => ok(game,{type:'query',name:'state',arguments:{}}).value;
const readonly = new BrowserRuntime(false);
assert.deepEqual(ok(readonly,{type:'capabilities'}).operations,['inspect','query','capture']);
for(const request of [{type:'step',frames:1},{type:'invoke',name:'place',arguments:{kind:'conveyor',x:2,y:3,facing:'E'}}]) {
  assert.equal(raw(readonly,request).error.code,'mutation_disabled');
}
const readonlyBefore = state(readonly);
const readonlyStatus = ok(readonly,{type:'status'});
const readonlyRecording = ok(readonly,{type:'query',name:'recording',arguments:{}});
for (let read=0; read<3; read++) {
  const ui = ok(readonly,{type:'query',name:'interface',arguments:{}}).value;
  assert.deepEqual(ui.structures, readonlyBefore.structures);
  const preview = ok(readonly,{type:'query',name:'preview',arguments:{x:2,y:3,action:'place'}}).value;
  assert.equal(preview.valid,true);
  assert.deepEqual(state(readonly),readonlyBefore);
}
assert.deepEqual(ok(readonly,{type:'status'}),readonlyStatus);
assert.deepEqual(ok(readonly,{type:'query',name:'recording',arguments:{}}),readonlyRecording);
readonly.free();
const game = new BrowserRuntime(true);
const initial = state(game);
assert.equal(initial.structures.length,1);
assert.deepEqual(initial.structures.map(({x,y,kind,facing})=>({x,y,kind,facing})),[{x:10,y:3,kind:'delivery',facing:'E'}]);
const capture = ok(game,{type:'capture'});
assert.ok(capture.artifact.startsWith('data:image/png;base64,'));
const operations=JSON.parse(readFileSync(resolve(root,'tests/construction.json'),'utf8'));
invoke(game,'sequence',{operations});
const built=state(game);
assert.equal(built.tick,60);
assert.equal(built.delivered,0);
assert.deepEqual(built.structures.map(({x,y,kind,facing})=>({x,y,kind,facing})),[
 {x:1,y:3,kind:'extractor',facing:'E'}, {x:2,y:3,kind:'conveyor',facing:'S'}, {x:10,y:3,kind:'delivery',facing:'E'}]);
assert.notEqual(ok(game,{type:'capture'}).checksum,capture.checksum);
for(const operation of [{op:'remove',x:10,y:3},{op:'rotate',x:0,y:0},{op:'place',kind:'processor',x:1,y:3,facing:'E'}, {op:'place',kind:'delivery',x:3,y:3,facing:'E'}, {op:'place',kind:'conveyor',x:3,y:3,facing:'bad'}]) {
 const before=state(game); const result=raw(game,{type:'invoke',name:'construct',arguments:operation});
 assert.equal(result.status,'failure'); assert.deepEqual(state(game),before);
}
const frame=ok(game,{type:'status'}).current_frame;
invoke(game,'restart');
assert.equal(ok(game,{type:'status'}).current_frame,frame);
assert.deepEqual(state(game).structures,initial.structures);
assert.deepEqual(state(game).camera,initial.camera);
invoke(game,'sequence',{operations});
assert.deepEqual(state(game).structures,built.structures);
assert.equal(state(game).tick,built.tick);
game.free();
console.log('Factory actual-WASM construction, rejection, deterministic sequence, read-only policy, capture and restart passed.');
const {transportAcceptance}=await import('./transport-acceptance.mjs');
await transportAcceptance({BrowserRuntime,root,target:metadata.target_directory,raw,ok,state});
const {productionAcceptance}=await import('./production-acceptance.mjs');
await productionAcceptance({BrowserRuntime,root,target:metadata.target_directory,raw,state});
const {interfaceAcceptance}=await import('./interface-acceptance.mjs');
interfaceAcceptance({BrowserRuntime,ok,state});
