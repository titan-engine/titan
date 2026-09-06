import assert from 'node:assert/strict';
import { inspectEntities, entityRow, inspectionDetails } from '../web/play/entities.mjs';
import { execFile } from './acceptance_process.mjs';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const metadata = JSON.parse(await execFile('cargo', ['metadata', '--locked', '--format-version', '1', '--no-deps'], { phase: 'build', cwd: root, encoding: 'utf8' }));
const { BrowserRuntime, BrowserLiveRuntime, verify_recording_json } = createRequire(import.meta.url)(resolve(metadata.target_directory, 'titan/browser-node/titan_game.js'));
let sequence = 0;
const envelope = request => ({ schema_version: 2, request_id: `test-${++sequence}`, request });
const raw = (runtime, request) => JSON.parse(runtime.handle(JSON.stringify(envelope(request))));
function ok(runtime, request) { const response = raw(runtime, request); assert.equal(response.status, 'success', JSON.stringify(response)); return response.response; }
function fail(runtime, request, code) { const response = raw(runtime, request); assert.equal(response.status, 'failure'); assert.equal(response.error.code, code); return response; }
const readonly = new BrowserRuntime(false);
assert.deepEqual(ok(readonly, { type: 'capabilities' }).operations, ['inspect', 'query', 'capture']);
assert.deepEqual(ok(readonly, { type: 'commands' }).commands, []);
for (const request of [
  { type: 'step', frames: 1 },
  { type: 'inject_input', frame: 1, actions: {} },
  { type: 'invoke', name: 'restart', arguments: {} },
  { type: 'invoke', name: 'ui_pointer', arguments: {x:8,y:12,pressed:true} },
  { type: 'set_field', entity: { index: 0, generation: 0 }, component: 'Position', field: 'x', value: 0 },
]) { assert.equal(fail(readonly, request, 'mutation_disabled').observed_frame, 0); }
for (const [change, code] of [[{ schema_version: 999 }, 'protocol_mismatch'], [{ target_instance: 'missing' }, 'not_found']]) {
  const result = JSON.parse(readonly.handle(JSON.stringify({ ...envelope({ type: 'step', frames: 1 }), ...change })));
  assert.equal(result.error.code, code);
}
assert.equal(JSON.parse(readonly.handle('no JSON')).error.code, 'invalid_value');
const inactiveEntities = inspectEntities(request => raw(readonly, request));
const panelDetails = inspectionDetails(raw(readonly, {type:'query', name:'arena_state'}), inactiveEntities);
const panelJson = JSON.stringify(panelDetails, null, 2);
assert.ok(panelJson.length <= 12000, `complete inspection JSON exceeds panel limit: ${panelJson.length}`);
assert.equal(JSON.parse(panelJson).entities.entities.length, 18);
assert.ok(JSON.parse(panelJson).entities.entities.some(entity => entity.name === 'ui/restart' && Object.keys(entity.components).some(name => name.endsWith('::UiText'))));

assert.equal(inactiveEntities.entities.length, 18);
assert.equal(inactiveEntities.truncated, false);
assert.equal(inactiveEntities.entities.filter(entity => entityRow(entity)[2] === 'Player').length, 1);
assert.equal(inactiveEntities.entities.filter(entity => entityRow(entity)[2] === 'Inactive · awaiting spawn').length, 14);
assert.deepEqual(inactiveEntities.entities.map(entity => entity.name), ['player', ...Array.from({length:14}, (_, index) => `enemy-${index}`), 'ui/status', 'ui/restart', 'ui/dash']);
const uiButton = inactiveEntities.entities.find(entity => entity.name === 'ui/restart');
const uiTextKey = Object.keys(uiButton.components).find(name => name.endsWith('::UiText'));
assert.equal(uiButton.components[uiTextKey].text, 'R RESTART');
assert.equal(uiButton.component_fields[uiTextKey].text.writable, false);
readonly.free();
const game = new BrowserRuntime(true);
assert.equal(ok(game, { type: 'capabilities' }).mutation_enabled, true);
const initial = ok(game, { type: 'capture' });
assert.equal(initial.checksum,'e096abf94fd12c24');
assert.ok(initial.artifact.startsWith('data:image/png;base64,'));
assert.deepEqual([...Buffer.from(initial.artifact.split(',')[1], 'base64').subarray(0, 8)], [137,80,78,71,13,10,26,10]);
const entity = ok(game, { type: 'entities', query: {}, page: { limit: 100 } }).entities.find(entity => entity.name === 'player').id;
const details = () => ok(game, { type: 'entity', entity });
const component = Object.keys(details().components).find(name => name.endsWith('::Position'));
assert.ok(component);
const position = () => details().components[component];
const before = position();
ok(game, { type: 'inject_input', frame: 1, actions: { right: { kind: 'button', value: true } } });
ok(game, { type: 'step', frames: 1 });
assert.equal(position().x, before.x + 1);
const pursuingEntities = inspectEntities(request => raw(game, request));
assert.equal(pursuingEntities.entities.length, 18);
assert.equal(pursuingEntities.entities.filter(entity => entityRow(entity)[2] === 'Active pursuer').length, 1);
assert.equal(pursuingEntities.entities.filter(entity => entityRow(entity)[2] === 'Inactive · awaiting spawn').length, 13);
assert.equal(ok(game, {type:'status'}).current_frame, 1, 'entity inspection does not advance simulation');

assert.notEqual(ok(game, { type: 'capture' }).checksum, initial.checksum);
const edit = value => ({ type: 'set_field', entity, component, field: 'x', value });
fail(game, edit(-1), 'invalid_value');
fail(game, edit('wrong'), 'invalid_value');
ok(game, edit(10));
assert.equal(position().x, 10);
assert.ok(ok(game, { type: 'commands' }).commands.some(command => command.name === 'restart'));
ok(game, { type: 'inject_input', frame: 2, actions: { right: { kind: 'button', value: true } } });
ok(game, { type: 'invoke', name: 'restart', arguments: {} });
assert.equal(ok(game, { type: 'status' }).current_frame, 1);
assert.equal(ok(game, { type: 'capture' }).checksum, initial.checksum);
ok(game, { type: 'step', frames: 1 });
assert.deepEqual(position(), before);
// Native and actual WASM share this exact dash trajectory and complete snapshots.
ok(game, { type: 'invoke', name: 'restart', arguments: {} });
const clock = ok(game, { type: 'status' }).current_frame;
for (let tick = 1; tick <= 121; tick++) {
  ok(game, { type: 'inject_input', frame: clock + tick, actions: { dash: { kind: 'button', value: true } } });
}
ok(game, { type: 'step', frames: 1 });
assert.deepEqual(position(), { x: 84, y: 65 });
const dashActive = ok(game, { type: 'capture' }).checksum;
assert.notEqual(dashActive, initial.checksum);
ok(game, { type: 'step', frames: 5 });
assert.deepEqual(position(), { x: 104, y: 65 });
const dashCooldown = ok(game, { type: 'capture' }).checksum;
assert.notEqual(dashCooldown, dashActive);
ok(game, { type: 'step', frames: 115 });
assert.deepEqual(position(), { x: 104, y: 65 }, 'held dash does not retrigger');
ok(game, { type: 'inject_input', frame: clock + 122, actions: { left: { kind: 'button', value: true } } });
ok(game, { type: 'step', frames: 1 });
ok(game, { type: 'inject_input', frame: clock + 123, actions: { dash: { kind: 'button', value: true } } });
ok(game, { type: 'step', frames: 1 });
assert.deepEqual(position(), { x: 99, y: 65 }, 'rearmed dash uses last movement');
ok(game, { type: 'inject_input', frame: clock + 124, actions: { dash: { kind: 'button', value: true } } });
ok(game, { type: 'invoke', name: 'restart', arguments: {} });
ok(game, { type: 'step', frames: 1 });
assert.deepEqual(position(), before, 'restart clears pending dash');
const clickFrame = ok(game, {type:'status'}).current_frame;
ok(game, {type:'invoke', name:'ui_pointer', arguments:{x:8,y:12,pressed:true}});
assert.equal(ok(game, {type:'query', name:'arena_state'}).value.run.elapsed, 1, 'press alone does not activate');
ok(game, {type:'invoke', name:'ui_pointer', arguments:{x:8,y:12,pressed:false}});
assert.equal(ok(game, {type:'status'}).current_frame, clickFrame);
assert.equal(ok(game, {type:'capture'}).checksum, initial.checksum);
assert.equal(ok(game, {type:'query', name:'recording'}).value.recorded_ticks, 0);
game.free();
const arena = new BrowserRuntime(true);
for (let tick=0; tick<1200; tick++) {
  const t=(tick-90+360)%360;
  const action=tick<30?'up':tick<90?'right':t<60?'down':t<180?'left':t<240?'up':'right';
  ok(arena,{type:'inject_input',frame:tick+1,actions:{[action]:{kind:'button',value:true}}});
}
ok(arena,{type:'step',frames:1200});
ok(arena,{type:'invoke',name:'verify_survival',arguments:{}});
assert.equal(ok(arena,{type:'capture'}).checksum,'b5cf61da6f50efd7');
const survivalRecording = ok(arena,{type:'query',name:'recording'}).value;
const survivalSave = ok(arena,{type:'query',name:'save'}).value;
ok(arena,{type:'invoke',name:'restart',arguments:{}});
ok(arena,{type:'step',frames:310});
fail(arena,{type:'invoke',name:'verify_survival',arguments:{}},'invalid_value');
arena.free();
console.log('Arena actual-WASM policy, deterministic survival/loss, input, dash/cooldown/held/rearm, capture, fields and restart checks passed.');

// Actual WASM session tests; the GPU BrowserPlayer has its own /test/ fixture.
const live = new BrowserLiveRuntime();
assert.equal(ok(live, {type:'status'}).current_frame, 0);
live.tick();
assert.equal(ok(live, {type:'status'}).current_frame, 0, 'starts paused');
live.resume();
live.set_action('right', true);
live.set_action('dash', true);
live.set_action('dash', false);
live.tick();
live.pause();
const visible = ok(live, {type:'query', name:'arena_state'}).value;
assert.deepEqual(visible.position, {x:84, y:65});
assert.equal(visible.run.dash_cooldown, 120);
fail(live, {type:'step', frames:1}, 'mutation_disabled');
const recording = ok(live, {type:'query', name:'recording'}).value;
const verified = JSON.parse(verify_recording_json(JSON.stringify(recording)));
assert.equal(verified.ticks, 1);
assert.throws(() => verify_recording_json(' '.repeat(2 * 1024 * 1024 + 1)), /2 MiB/);
live.set_control_enabled(true);
assert.deepEqual(ok(live, {type:'query', name:'arena_state'}).value.position, visible.position, 'opt-in keeps live scene');
ok(live, {type:'step', frames:1});
assert.equal(ok(live, {type:'status'}).current_frame, 2);
live.set_control_enabled(false);
fail(live, {type:'step', frames:1}, 'mutation_disabled');
const beforePointer = ok(live, {type:'query', name:'arena_state'}).value;
assert.equal(live.pointer(8, 12, true), true);
assert.equal(ok(live, {type:'query', name:'arena_state'}).value.run.elapsed, beforePointer.run.elapsed);
live.pointer(100, 50, false);
assert.equal(ok(live, {type:'query', name:'arena_state'}).value.run.elapsed, beforePointer.run.elapsed, 'release outside cancels');
live.pointer(8, 12, true); live.cancel_pointer(); live.pointer(8, 12, false);
assert.equal(ok(live, {type:'query', name:'arena_state'}).value.run.elapsed, beforePointer.run.elapsed, 'canceled gesture cannot restart');
live.pointer(8, 12, true); live.pointer(8, 12, false);
const restarted = ok(live, {type:'query', name:'arena_state'}).value;
assert.equal(restarted.run.elapsed, 0, 'local ECS button works paused with readonly inspection');
assert.equal(restarted.frame, beforePointer.frame, 'button preserves host clock');
assert.equal(restarted.paused, true);
assert.equal(restarted.recording.recorded_ticks, 0);
assert.equal(ok(live, {type:'capture'}).checksum, 'e096abf94fd12c24');
live.free();
console.log('live headless WASM session: read-only inspection, opt-in, same-instance step and exact recording replay passed');

// Mid-dash persistence uses the same paused live session and clears transient input.
const sourceSaveGame = new BrowserLiveRuntime();
sourceSaveGame.resume();
sourceSaveGame.set_action('right', true);
sourceSaveGame.set_action('dash', true);
sourceSaveGame.tick();
sourceSaveGame.pause();
const saveStatusBefore = raw(sourceSaveGame, {type:'status'});
const saveData = ok(sourceSaveGame, {type:'query', name:'save'}).value;
const saveImage = ok(sourceSaveGame, {type:'capture'}).checksum;
assert.ok(Buffer.byteLength(JSON.stringify(saveData)) < 64 * 1024);
assert.equal(raw(sourceSaveGame, {type:'status'}).state_revision, saveStatusBefore.state_revision, 'save is read-only');
const loadRequest = save => ({type:'invoke', name:'load_save', arguments:{save}});
fail(sourceSaveGame, loadRequest(saveData), 'mutation_disabled');

const restoredSaveGame = new BrowserLiveRuntime();
restoredSaveGame.set_control_enabled(true);
restoredSaveGame.resume();
for (let tick = 0; tick < 3; tick++) restoredSaveGame.tick();
fail(restoredSaveGame, loadRequest(saveData), 'not_controlled');
restoredSaveGame.pause();
const invalidBefore = raw(restoredSaveGame, {type:'status'});
const invalidState = ok(restoredSaveGame, {type:'query', name:'save'}).value;
for (const invalidSave of [{}, [], {...saveData, format_version:999}, {padding:'x'.repeat(64 * 1024)}]) {
  const rejected = fail(restoredSaveGame, loadRequest(invalidSave), 'invalid_value');
  assert.equal(rejected.observed_frame, invalidBefore.observed_frame);
  assert.equal(rejected.state_revision, invalidBefore.state_revision);
  assert.deepEqual(ok(restoredSaveGame, {type:'query', name:'save'}).value, invalidState);
}
const loadFrame = ok(restoredSaveGame, {type:'status'}).current_frame;
ok(restoredSaveGame, {type:'inject_input', frame:loadFrame+1, actions:{left:{kind:'button',value:true}}});
restoredSaveGame.set_action('down', true);
restoredSaveGame.set_action('dash', true);
restoredSaveGame.pointer(8, 12, true);
const loadBefore = raw(restoredSaveGame, {type:'status'});
const loaded = raw(restoredSaveGame, loadRequest(saveData));
assert.equal(loaded.status, 'success');
assert.equal(loaded.observed_frame, loadFrame);
assert.ok(loaded.state_revision > loadBefore.state_revision);
restoredSaveGame.pointer(8, 12, false);
assert.deepEqual(ok(restoredSaveGame, {type:'query', name:'save'}).value, saveData, 'loaded state and canceled pointer preserve snapshot');
assert.equal(ok(restoredSaveGame, {type:'capture'}).checksum, saveImage, 'rebuilt HUD and world match');
assert.equal(ok(restoredSaveGame, {type:'query', name:'arena_state'}).value.paused, true);
const loadedRecording = ok(restoredSaveGame, {type:'query', name:'recording'}).value;
assert.equal(loadedRecording.invalid_reason, null);
assert.equal(loadedRecording.recorded_ticks, 0);
assert.equal(JSON.parse(verify_recording_json(JSON.stringify(loadedRecording))).checksum, saveImage, 'loaded snapshot is an exact zero-input recording origin');
sourceSaveGame.resume(); restoredSaveGame.resume();
sourceSaveGame.tick(); restoredSaveGame.tick();
assert.deepEqual(ok(restoredSaveGame, {type:'query', name:'save'}).value, ok(sourceSaveGame, {type:'query', name:'save'}).value, 'load clears held and scheduled input');
for (let tick = 0; tick < 90; tick++) {
  for (const action of ['up','down','left','right','dash']) {
    const pressed = action === (tick < 30 ? 'up' : 'right');
    sourceSaveGame.set_action(action, pressed);
    restoredSaveGame.set_action(action, pressed);
  }
  sourceSaveGame.tick(); restoredSaveGame.tick();
}
sourceSaveGame.pause(); restoredSaveGame.pause();
assert.deepEqual(ok(restoredSaveGame, {type:'query', name:'save'}).value, ok(sourceSaveGame, {type:'query', name:'save'}).value);
assert.equal(ok(restoredSaveGame, {type:'capture'}).checksum, ok(sourceSaveGame, {type:'capture'}).checksum);
sourceSaveGame.free(); restoredSaveGame.free();
console.log('arena save/load actual-WASM: bounded validation, paused policy, monotonic host clock, input reset and deterministic continuation passed');

// Playback starts at a real mid-dash snapshot, then consumes the recorded edges only.
const replaySource = new BrowserLiveRuntime();
replaySource.set_control_enabled(true);
ok(replaySource, loadRequest(saveData));
replaySource.resume();
for (const action of ['left','up','up','right','down','left','up','right']) {
  for (const name of ['left','right','up','down']) replaySource.set_action(name, name === action);
  replaySource.tick();
}
replaySource.pause();
const snapshotRecording = ok(replaySource, {type:'query', name:'recording'}).value;
assert.equal(snapshotRecording.format_version, 2);
assert.deepEqual(snapshotRecording.initial_snapshot, saveData);
const snapshotVerification = JSON.parse(verify_recording_json(JSON.stringify(snapshotRecording)));
assert.deepEqual(snapshotVerification.save, ok(replaySource, {type:'query', name:'save'}).value);
assert.equal(snapshotVerification.checksum, ok(replaySource, {type:'capture'}).checksum);
const legacyRecording = JSON.parse(readFileSync(resolve(root, 'tests/fixtures/recording-v1.json'), 'utf8'));
const legacyVerification = JSON.parse(verify_recording_json(JSON.stringify(legacyRecording)));
assert.equal(legacyVerification.ticks, 194);
assert.equal(legacyVerification.checksum, 'ae923e36040921f9');
const playback = new BrowserLiveRuntime();
const replayState = () => ok(playback, {type:'query', name:'arena_state'}).value.replay;
const replayCommand = (name, args = {}) => ({type:'invoke', name, arguments:args});
fail(playback, replayCommand('load_replay', {recording:snapshotRecording}), 'mutation_disabled');
// Local playback needs pause but does not grant remote mutation permission.
playback.resume();
assert.throws(() => playback.load_recording(JSON.stringify(snapshotRecording)), /pause/i);
playback.pause();
assert.throws(() => playback.load_recording(' '.repeat(2 * 1024 * 1024 + 1)), /2 MiB/);
playback.load_recording(JSON.stringify(snapshotRecording));
assert.equal(replayState().active, true);
assert.equal(replayState().position, 0);
assert.deepEqual(ok(playback, {type:'query', name:'save'}).value, saveData);
playback.step_playback();
assert.equal(replayState().position, 1);
playback.restart_playback();
assert.equal(replayState().position, 0);
playback.set_control_enabled(true);
const unchanged = () => ({status:raw(playback,{type:'status'}), save:ok(playback,{type:'query',name:'save'}).value, replay:replayState()});
for (const rejectedRequest of [
  replayCommand('load_replay', {recording:{}}),
  replayCommand('load_replay', {recording:{...snapshotRecording, final_checksum:'0000000000000000'}}),
  {type:'step',frames:9},
  {type:'inject_input',frame:ok(playback,{type:'status'}).current_frame+1,actions:{left:{kind:'button',value:true}}},
  {type:'set_field',entity,component,field:'x',value:10},
  loadRequest(saveData),
  replayCommand('ui_pointer',{x:8,y:12,pressed:true}),
]) {
  const beforeRejected = unchanged();
  const rejected = raw(playback, rejectedRequest);
  assert.equal(rejected.status, 'failure', JSON.stringify(rejectedRequest));
  const afterRejected = unchanged();
  assert.equal(afterRejected.status.observed_frame, beforeRejected.status.observed_frame);
  assert.equal(afterRejected.status.state_revision, beforeRejected.status.state_revision);
  assert.deepEqual(afterRejected.save, beforeRejected.save);
  assert.deepEqual(afterRejected.replay, beforeRejected.replay);
}
for (const action of ['left','right','up','down','dash']) playback.set_action(action,true);
playback.pointer(8,12,true); playback.pointer(8,12,false);
assert.equal(replayState().position, 0, 'live pointer cannot restart playback');
ok(playback,{type:'step',frames:3});
const restartFrame = ok(playback,{type:'status'}).current_frame;
ok(playback,replayCommand('restart_replay'));
assert.equal(ok(playback,{type:'status'}).current_frame,restartFrame);
assert.deepEqual(ok(playback,{type:'query',name:'save'}).value,saveData);
playback.resume();
for(let tick=0;tick<20;tick++) playback.tick();
assert.equal(ok(playback,{type:'status'}).current_frame,restartFrame+8, 'EOF prevents overshoot');
assert.equal(ok(playback,{type:'status'}).paused,true);
assert.equal(replayState().position,8);
assert.equal(replayState().complete,true);
assert.equal(replayState().verified,true);
assert.deepEqual(ok(playback,{type:'query',name:'save'}).value,snapshotVerification.save);
assert.equal(ok(playback,{type:'capture'}).checksum,snapshotVerification.checksum);
const eofFrame = ok(playback,{type:'status'}).current_frame;
playback.resume(); playback.tick();
assert.equal(ok(playback,{type:'status'}).current_frame,eofFrame);
playback.pause();
ok(playback,replayCommand('stop_replay'));
assert.equal(replayState().active,false);
assert.equal(ok(playback,{type:'capture'}).checksum,'e096abf94fd12c24', 'stop replay starts a fresh live run');
assert.equal(ok(playback,{type:'status'}).current_frame,eofFrame);
assert.equal(JSON.parse(verify_recording_json(JSON.stringify(ok(playback,{type:'query',name:'recording'}).value))).checksum,'e096abf94fd12c24');
ok(playback,replayCommand('load_replay',{recording:snapshotRecording}));
ok(playback,replayCommand('restart'));
assert.equal(replayState().active,false);
assert.equal(ok(playback,{type:'capture'}).checksum,'e096abf94fd12c24', 'normal restart exits replay');
ok(playback,replayCommand('load_replay',{recording:legacyRecording}));
ok(playback,{type:'step',frames:194});
assert.equal(replayState().verified,true);
assert.deepEqual(ok(playback,{type:'query',name:'save'}).value,legacyVerification.save);
assert.equal(ok(playback,{type:'capture'}).checksum,legacyVerification.checksum);
playback.free(); replaySource.free();
console.log('actual-WASM snapshot/v1 replay: full-state verification, local playback, isolated inputs, pause/step/restart, monotonic clock and EOF auto-pause passed');

// Seeking replays only recorded inputs and spreads long reconstruction over updates.
const seeking = new BrowserLiveRuntime();
seeking.load_recording(JSON.stringify(survivalRecording));
const seekState = () => ok(seeking, {type:'query',name:'arena_state'}).value.replay;
const seekSave = () => ok(seeking, {type:'query',name:'save'}).value;
function finishSeek(position) {
  let updates = 0;
  while (seekState().position !== position) {
    const before = seekState().position;
    seeking.update_playback();
    const after = seekState().position;
    assert.ok(after > before && after - before <= 120, 'each seek update has a fixed tick budget');
    assert.ok(++updates <= 10, 'seek must finish within its bounded number of updates');
  }
  assert.equal(ok(seeking,{type:'status'}).paused, true, 'seek completion remains paused');
}
fail(seeking,replayCommand('seek_replay',{position:600}), 'mutation_disabled');
fail(seeking,replayCommand('replay_speed',{speed:2}), 'mutation_disabled');
seeking.set_control_enabled(true);
for (const speed of [0, -1, 0.1, 8, NaN, Infinity]) {
  const before = seekState();
  assert.throws(() => seeking.set_playback_speed(speed));
  assert.deepEqual(seekState(),before, 'invalid speed is transactional');
}
for (const position of [-1, 1201, 1.5]) {
  const before = seekState();
  fail(seeking,replayCommand('seek_replay',{position}),'invalid_value');
  assert.deepEqual(seekState(),before, 'invalid seek is transactional');
}
seeking.set_playback_speed(4);
seeking.seek_playback(600);
assert.equal(seekState().position,0,'request does not synchronously reconstruct a large seek');
for (const action of ['left','right','up','down','dash']) seeking.set_action(action,true);
seeking.pointer(8,12,true); seeking.pointer(8,12,false);
seeking.update_playback();
assert.equal(seekState().position,120,'one update processes only 120 recorded ticks');
finishSeek(600);
const midwaySave = seekSave();
const midwayImage = ok(seeking,{type:'capture'}).checksum;
seeking.seek_playback(0);
finishSeek(0);
assert.equal(ok(seeking,{type:'capture'}).checksum,'e096abf94fd12c24');
seeking.set_playback_speed(0.5);
ok(seeking,{type:'step',frames:600});
assert.deepEqual(seekSave(),midwaySave,'seek equals sequential replay at the same position');
assert.equal(ok(seeking,{type:'capture'}).checksum,midwayImage);
ok(seeking,replayCommand('seek_replay',{position:50}));
finishSeek(50);
ok(seeking,replayCommand('replay_speed',{speed:2}));
ok(seeking,replayCommand('seek_replay',{position:1200}));
finishSeek(1200);
assert.equal(seekState().verified,true);
assert.deepEqual(seekSave(),survivalSave);
assert.equal(ok(seeking,{type:'capture'}).checksum,'b5cf61da6f50efd7');
seeking.seek_playback(0); finishSeek(0);
seeking.set_playback_speed(1);
seeking.resume();
for (let tick=0;tick<1200;tick++) seeking.tick();
assert.equal(seekState().verified,true,'playback after seeking still isolates held live input');
assert.deepEqual(seekSave(),survivalSave);
assert.equal(ok(seeking,{type:'capture'}).checksum,'b5cf61da6f50efd7');
seeking.free();
console.log('actual-WASM bounded forward/backward seeking, speed validation, live input isolation and exact canonical final snapshot/pixels passed');


// The public asynchronous boundary accepts before returning and releases the
// WASM mutable borrow: another request and free are legal before resolution.
{
  const asynchronous = new BrowserRuntime(true);
  const capture = asynchronous.dispatch(JSON.stringify({ schema_version: 2, request_id: 'promise-capture', request: { type: 'capture' } }));
  assert.ok(capture instanceof Promise);
  const step = asynchronous.dispatch(JSON.stringify({ schema_version: 2, request_id: 'promise-step', request: { type: 'step', frames: 1 } }));
  asynchronous.free();
  const [captured, stepped] = (await Promise.all([capture, step])).map(JSON.parse);
  assert.equal(captured.request_id, 'promise-capture');
  assert.equal(captured.status, 'success');
  assert.equal(captured.observed_frame, 0);
  assert.equal(captured.response.identity.observed_frame, 0);
  assert.equal(stepped.observed_frame, 1);
  assert.equal(captured.response.identity.instance_id, captured.instance_id);
}
