import assert from 'node:assert/strict';
import { execFile } from './acceptance_process.mjs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const metadata = JSON.parse(await execFile('cargo', ['metadata', '--format-version', '1', '--no-deps'], { phase: 'build', cwd: root, encoding: 'utf8' }));
const { BrowserRuntime } = createRequire(import.meta.url)(resolve(metadata.target_directory, 'titan/browser-node/titan_game.js'));
let sequence = 0;
const envelope = request => ({ schema_version: 1, request_id: `test-${++sequence}`, request });
const raw = (runtime, request) => JSON.parse(runtime.handle(JSON.stringify(envelope(request))));
function ok(runtime, request) { const response = raw(runtime, request); assert.equal(response.status, 'success', JSON.stringify(response)); return response.response; }
function fail(runtime, request, code) { const response = raw(runtime, request); assert.equal(response.status, 'failure'); assert.equal(response.error.code, code); return response; }
const readonly = new BrowserRuntime(false);
assert.deepEqual(ok(readonly, { type: 'capabilities' }).operations, ['inspect', 'capture']);
assert.deepEqual(ok(readonly, { type: 'commands' }).commands, []);
for (const request of [
  { type: 'step', frames: 1 },
  { type: 'inject_input', frame: 1, actions: {} },
  { type: 'invoke', name: 'restart', arguments: {} },
  { type: 'set_field', entity: { index: 0, generation: 0 }, component: 'Position', field: 'x', value: 0 },
]) { assert.equal(fail(readonly, request, 'mutation_disabled').observed_frame, 0); }
for (const [change, code] of [[{ schema_version: 999 }, 'protocol_mismatch'], [{ target_instance: 'missing' }, 'not_found']]) {
  const result = JSON.parse(readonly.handle(JSON.stringify({ ...envelope({ type: 'step', frames: 1 }), ...change })));
  assert.equal(result.error.code, code);
}
assert.equal(JSON.parse(readonly.handle('no JSON')).error.code, 'invalid_value');
readonly.free();
const game = new BrowserRuntime(true);
assert.equal(ok(game, { type: 'capabilities' }).mutation_enabled, true);
const initial = ok(game, { type: 'capture' });
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
game.free();
console.log('Starter actual-WASM policy, input, capture, fields and restart checks passed.');
