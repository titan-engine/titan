// Real WASM RPG sessions, with exported recordings verified by a native process.
import assert from 'node:assert/strict';
import { execFile, run } from './acceptance_process.mjs';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const metadata = JSON.parse(await execFile('cargo', ['metadata', '--no-deps', '--format-version', '1'], {phase:'build',cwd:root, encoding:'utf8'}));
const {BrowserLiveRuntime, verify_recording_json} = createRequire(import.meta.url)(resolve(metadata.target_directory, 'titan/browser-node/titan_browser.js'));
let sequence = 0;
function raw(game, request) {
  return JSON.parse(game.handle(JSON.stringify({schema_version:1, request_id:`rpg-replay-${++sequence}`, request})));
}
function ok(game, request) {
  const result = raw(game, request);
  assert.equal(result.status, 'success', JSON.stringify(result));
  return result.response;
}
const query = (game, name) => ok(game, {type:'query', name}).value;
const invoke = (name, arguments_ = {}) => ({type:'invoke', name, arguments:arguments_});
const capture = game => ok(game, {type:'capture'}).checksum;
const status = game => raw(game, {type:'status'});
const entities = game => ok(game, {type:'entities'}).entities;
const gameplayEntities = game => entities(game).filter(entity => !entity.name?.startsWith('ui/journal/'));
const journal = game => query(game,'rpg_state').journal;
const journalKey = (game,key) => ok(game,invoke('journal_key',{key}));
function uiText(game) {
  const entity = entities(game).find(entity => entity.name === 'ui/quest').id;
  const components = ok(game, {type:'entity',entity}).components;
  return Object.entries(components).find(([key]) => key.endsWith('::UiText'))[1].text;
}
function route(game, actions) {
  const frame = status(game).observed_frame;
  actions.forEach((action,index) => ok(game, {type:'inject_input',frame:frame+index+1,actions:{[action]:{kind:'button',value:true}}}));
  ok(game, {type:'step',frames:actions.length});
}
const source = new BrowserLiveRuntime();
const initial = query(source, 'save');
const initialChecksum = capture(source);
const initialVerified = JSON.parse(verify_recording_json(JSON.stringify(query(source,'recording'))));
assert.deepEqual(initialVerified.save,initial);
assert.equal(initialVerified.checksum,initialChecksum);
assert.equal(uiText(source), 'SHARDS 0/3');
assert.equal(raw(source, invoke('load_save',{save:initial})).error.code,'mutation_disabled');
source.set_control_enabled(true);
assert.deepEqual(journal(source),{open:false,selected:'shards',focused:null});
const journalFrame = status(source).observed_frame;
journalKey(source,'toggle');
assert.deepEqual(journal(source),{open:true,selected:'shards',focused:'ui/journal/shards'});
assert.notEqual(capture(source),initialChecksum);
assert.deepEqual(query(source,'save'),initial);
const modalPlayer = entities(source).find(entity=>entity.name==='player').id;
const modalPosition = Object.keys(ok(source,{type:'entity',entity:modalPlayer}).components).find(key=>key.endsWith('::Position'));
for (const request of [{type:'step',frames:1}, {type:'inject_input',frame:journalFrame+1,actions:{}},
  {type:'set_field',entity:modalPlayer,component:modalPosition,field:'x',value:0}]) {
  const before = status(source);
  assert.equal(raw(source,request).status,'failure');
  assert.equal(status(source).observed_frame,before.observed_frame);
  assert.equal(status(source).state_revision,before.state_revision);
  assert.deepEqual(query(source,'save'),initial);
}
journalKey(source,'next');
assert.deepEqual(journal(source),{open:true,selected:'shrine',focused:'ui/journal/shrine'});
journalKey(source,'previous');
assert.equal(journal(source).selected,'shards');
journalKey(source,'previous');
assert.equal(journal(source).focused,'ui/journal/close');
journalKey(source,'activate');
assert.equal(journal(source).open,false);
assert.equal(ok(source,{type:'status'}).paused,true);
assert.equal(capture(source),initialChecksum);
assert.equal(status(source).observed_frame,journalFrame);
for (const pressed of [true,false]) ok(source,invoke('journal_pointer',{x:5,y:5,pressed}));
assert.equal(journal(source).open,true);
for (const pressed of [true,false]) ok(source,invoke('journal_pointer',{x:20,y:88,pressed}));
assert.equal(journal(source).open,false);
assert.equal(capture(source),initialChecksum);
// Opening while running freezes actual browser ticks and closing restores running state.
source.resume();
journalKey(source,'toggle');
for(let tick=0;tick<3;tick++) source.tick();
assert.equal(status(source).observed_frame,journalFrame);
assert.equal(ok(source,{type:'status'}).paused,true);
journalKey(source,'close');
assert.equal(ok(source,{type:'status'}).paused,false);
source.pause();
route(source, ['right','right']);
const middle = query(source,'save');
const middleChecksum = capture(source);
assert.deepEqual(middle.player,{x:4,y:2});
assert.equal(middle.collected_shards,1);
assert.equal(middle.shards.length,2);
assert.equal(middle.shrine_active,false);
assert.equal(uiText(source),'SHARDS 1/3');
route(source, [...Array(3).fill('down'),...Array(6).fill('right')]);
const final = query(source,'save');
assert.equal(final.collected_shards,3);
assert.equal(final.shrine_active,true);
assert.equal(final.shards.length,0);
assert.equal(capture(source),'f7a298f62ad75c1c');
assert.equal(uiText(source),'SHARDS 3/3  SHRINE ACTIVE');
for(const save of [{}, [], {...middle,format_version:999}, {...middle,collected_shards:-1}]) {
  const before = status(source);
  assert.equal(raw(source,invoke('load_save',{save})).status,'failure');
  const after = status(source);
  assert.equal(after.observed_frame,before.observed_frame);
  assert.equal(after.state_revision,before.state_revision);
  assert.deepEqual(query(source,'save'),final);
}
ok(source,invoke('load_save',{save:middle}));
assert.deepEqual(query(source,'save'),middle);
assert.equal(capture(source),middleChecksum);
assert.equal(gameplayEntities(source).length,5);
route(source,[...Array(3).fill('down'),...Array(6).fill('right')]);
journalKey(source,'toggle');
assert.notEqual(capture(source),'f7a298f62ad75c1c');
const recording = query(source,'recording');
assert.deepEqual(recording.initial_snapshot,middle);
const verified = JSON.parse(verify_recording_json(JSON.stringify(recording)));
assert.deepEqual(verified.save,final);
assert.equal(verified.checksum,'f7a298f62ad75c1c');
assert.equal(recording.final_checksum,verified.checksum);
assert.equal(journal(source).open,true);
journalKey(source,'close');
assert.equal(capture(source),verified.checksum);

const playback = new BrowserLiveRuntime();
const replay = () => query(playback,'rpg_state').replay;
assert.equal(raw(playback,invoke('load_replay',{recording})).error.code,'mutation_disabled');
const beforeResume = status(playback);
playback.resume();
assert.ok(status(playback).state_revision > beforeResume.state_revision);
assert.throws(() => playback.load_recording(JSON.stringify(recording)),/pause/i);
const beforePause = status(playback);
playback.pause();
assert.ok(status(playback).state_revision > beforePause.state_revision);
assert.throws(() => playback.load_recording(' '.repeat(2*1024*1024+1)),/2 MiB/);
playback.load_recording(JSON.stringify(recording));
assert.deepEqual(query(playback,'save'),middle);
assert.equal(capture(playback),middleChecksum);
playback.set_control_enabled(true);
const replayJournalFrame = status(playback).observed_frame;
journalKey(playback,'toggle');
assert.equal(journal(playback).open,true);
assert.equal(replay().position,0);
assert.throws(()=>playback.step_playback(),/journal/i);
journalKey(playback,'close');
assert.equal(status(playback).observed_frame,replayJournalFrame);
assert.equal(capture(playback),middleChecksum);
playback.step_playback();
assert.equal(replay().position,1);
const localFrame = status(playback).observed_frame;
playback.restart_playback();
assert.equal(status(playback).observed_frame,localFrame);
assert.equal(replay().position,0);
playback.set_control_enabled(true);
const player = entities(playback).find(entity=>entity.name==='player').id;
const position = Object.keys(ok(playback,{type:'entity',entity:player}).components).find(key=>key.endsWith('::Position'));
const arenaRecording = JSON.parse(readFileSync(resolve(root,'games/arena/tests/fixtures/recording-v1.json'),'utf8'));
assert.throws(()=>verify_recording_json(JSON.stringify(arenaRecording)));
for(const request of [
  {type:'step',frames:10},
  {type:'inject_input',frame:localFrame+1,actions:{}},
  {type:'set_field',entity:player,component:position,field:'x',value:0},
  invoke('spawn_shard',{x:0,y:0}),
  invoke('load_save',{save:initial}),
  invoke('load_replay',{recording:{}}),
  invoke('load_replay',{recording:{...recording,final_checksum:'0000000000000000'}}),
  invoke('load_replay',{recording:arenaRecording}),
]) {
  const before = status(playback);
  assert.equal(raw(playback,request).status,'failure',JSON.stringify(request));
  const after = status(playback);
  assert.equal(after.observed_frame,before.observed_frame);
  assert.equal(after.state_revision,before.state_revision);
  assert.deepEqual(query(playback,'save'),middle);
  assert.equal(replay().position,0);
}
for(const action of ['up','down','left','right']) playback.set_action(action,true);
ok(playback,{type:'step',frames:3});
assert.equal(uiText(playback),'SHARDS 2/3');
const restartFrame = status(playback).observed_frame;
ok(playback,invoke('restart_replay'));
assert.equal(status(playback).observed_frame,restartFrame);
assert.deepEqual(query(playback,'save'),middle);
playback.resume();
for(let tick=0;tick<30;tick++) playback.tick();
assert.equal(status(playback).observed_frame,restartFrame+9);
assert.equal(ok(playback,{type:'status'}).paused,true);
assert.equal(replay().complete,true);
assert.equal(replay().verified,true);
assert.deepEqual(query(playback,'save'),verified.save);
assert.equal(capture(playback),verified.checksum);
playback.resume(); playback.tick();
assert.equal(status(playback).observed_frame,restartFrame+9);
playback.pause(); playback.exit_playback();
assert.equal(replay().active,false);
assert.deepEqual(query(playback,'save'),initial);
assert.equal(capture(playback),initialChecksum);
assert.equal(gameplayEntities(playback).length,6);
assert.equal(uiText(playback),'SHARDS 0/3');

journalKey(playback,'toggle');
journalKey(playback,'next');
assert.equal(journal(playback).selected,'shrine');
ok(playback,invoke('load_save',{save:initial}));
assert.deepEqual(journal(playback),{open:false,selected:'shards',focused:null});
assert.equal(capture(playback),initialChecksum);

// Export the actual WASM-consumed recording and verify it in native headless Rust.
const evidence = resolve(metadata.target_directory,'rpg-replay-evidence');
mkdirSync(evidence,{recursive:true});
const path = resolve(evidence,'wasm-recording.json');
writeFileSync(path,JSON.stringify(recording,null,2)+'\n');
await execFile('cargo',['build','--example','replay_rpg'],{phase:'build',cwd:root,stdio:'inherit'});
const native = JSON.parse(await execFile(resolve(metadata.target_directory,'debug/examples/replay_rpg'),[path],{cwd:root,encoding:'utf8'}));
assert.deepEqual(native.save,verified.save);
assert.equal(native.checksum,verified.checksum);
const foreign = await run(resolve(metadata.target_directory,'debug/examples/replay_rpg'),[resolve(root,'games/arena/tests/fixtures/recording-v1.json')],{encoding:'utf8'});
assert.notEqual(foreign.status,0);
ok(source,invoke('spawn_shard',{x:0,y:0}));
const editedRecording = query(source,'recording');
assert.match(editedRecording.invalid_reason,/outside consumed input/);
assert.throws(()=>verify_recording_json(JSON.stringify(editedRecording)),/invalidated/);
source.free(); playback.free();
console.log('RPG actual-WASM/native replay: initial and midquest snapshots, shard/shrine restoration, full state/pixels, isolated playback and EOF passed');
