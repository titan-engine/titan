import assert from 'node:assert/strict';
import { inspectEntities, entityRow } from '../web/play/entities.mjs';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: root, encoding: 'utf8' }));
const { BrowserRuntime, BrowserLiveRuntime, verify_recording_json } = createRequire(import.meta.url)(resolve(metadata.target_directory, 'titan/browser-node/titan_game.js'));
let sequence = 0;
const envelope = request => ({ schema_version: 1, request_id: `test-${++sequence}`, request });
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
