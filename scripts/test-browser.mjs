// Runs the real WASM build, not a mock or host-compiled substitute.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
const repo = fileURLToPath(new URL('../', import.meta.url));
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: repo, encoding: 'utf8' }));
const require = createRequire(import.meta.url);
const { BrowserRuntime } = require(resolve(metadata.target_directory, 'titan/browser-node/titan_browser.js'));
let sequence = 0;
const call = (runtime, request, success = true) => {
  const request_id = `wasm-${++sequence}`;
  const result = JSON.parse(runtime.handle(JSON.stringify({ schema_version: 1, request_id, request })));
  assert.equal(result.request_id, request_id);
  assert.equal(result.status, success ? 'success' : 'failure', JSON.stringify(result));
  return result;
};
const readOnly = new BrowserRuntime(false);
const capabilities = call(readOnly, { type: 'capabilities' }).response;
assert.equal(capabilities.run_mode, 'browser');
for (const operation of ['step', 'invoke', 'inject_input', 'mutate']) assert.ok(!capabilities.operations.includes(operation));
for (const request of [
  { type: 'step', frames: 1 },
  { type: 'invoke', name: 'spawn_shard', arguments: { x: 0, y: 0 } },
  { type: 'inject_input', frame: 1, actions: {} },
  { type: 'set_field', entity: { index: 0, generation: 0 }, component: 'Position', field: 'x', value: 0 },
]) {
  const result = call(readOnly, request, false);
  assert.equal(result.observed_frame, 0);
  assert.equal(result.state_revision, 0);
}
assert.equal(call(readOnly, { type: 'entities' }).response.entities.length, 5);
assert.equal(call(readOnly, { type: 'capture' }).response.format, 'png');
readOnly.free();

const runtime = new BrowserRuntime(true);
const supported = call(runtime, { type: 'capabilities' }).response.operations;
for (const operation of ['invoke', 'step', 'inject_input', 'capture']) assert.ok(supported.includes(operation));
const entities = call(runtime, { type: 'entities' }).response.entities;
const shrine = entities.find(entity => entity.name === 'shrine').id;
assert.equal(call(runtime, { type: 'commands' }).response.commands[0].name, 'spawn_shard');
let frame = 0;
for (const [action, ticks] of [['right', 2], ['down', 3], ['right', 6]]) {
  for (let tick = 0; tick < ticks; tick++) {
    const result = call(runtime, { type: 'inject_input', frame: ++frame, actions: { [action]: { kind: 'button', value: true } } });
    assert.equal(result.observed_frame, 0);
  }
}
const stepped = call(runtime, { type: 'step', frames: 11 });
assert.equal(stepped.observed_frame, 11);
const details = call(runtime, { type: 'entity', entity: shrine }).response;
assert.ok(Object.keys(details.components).some(name => name.endsWith('::ActiveShrine')));
assert.equal(call(runtime, { type: 'entities' }).response.entities.length, 2);
const capture = call(runtime, { type: 'capture' }).response;
assert.equal(capture.checksum, '98618cd721c5b52d');
assert.deepEqual([capture.width, capture.height], [160, 112]);
assert.ok(capture.artifact.startsWith('data:image/png;base64,'));
const png = Buffer.from(capture.artifact.split(',')[1], 'base64');
assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
const rejected = call(runtime, { type: 'invoke', name: 'spawn_shard', arguments: { x: -1, y: 0 } }, false);
assert.equal(rejected.state_revision, stepped.state_revision);
call(runtime, { type: 'invoke', name: 'spawn_shard', arguments: { x: 0, y: 0 } });
assert.notEqual(call(runtime, { type: 'capture' }).response.checksum, capture.checksum);
assert.equal(call(runtime, { type: 'status' }).observed_frame, 11);
const mismatch = JSON.parse(runtime.handle(JSON.stringify({ schema_version: 999, request_id: 'mismatch', request: { type: 'status' } })));
assert.equal(mismatch.error.code, 'protocol_mismatch');
assert.equal(mismatch.request_id, 'mismatch');
runtime.free();
console.log('WASM browser control loop passed: read-only policy, replay, exact capture, command, schema errors.');
