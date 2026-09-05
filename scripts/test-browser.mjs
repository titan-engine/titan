// Runs the real WASM build, not a mock or host-compiled substitute.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { execFile } from './acceptance_process.mjs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
const repo = fileURLToPath(new URL('../', import.meta.url));
const metadata = JSON.parse(await execFile('cargo', ['metadata', '--no-deps', '--format-version', '1'], { phase: 'build', cwd: repo, encoding: 'utf8' }));
const require = createRequire(import.meta.url);
const { BrowserRuntime } = require(resolve(metadata.target_directory, 'titan/browser-node/titan_browser.js'));
let sequence = 0;
const call = (runtime, request, success = true) => {
  const request_id = `wasm-${++sequence}`;
  const result = JSON.parse(runtime.handle(JSON.stringify({ schema_version: 2, request_id, request })));
  assert.equal(result.request_id, request_id);
  assert.equal(result.status, success ? 'success' : 'failure', JSON.stringify(result));
  return result;
};
const readOnly = new BrowserRuntime(false);
const capabilities = call(readOnly, { type: 'capabilities' }).response;
assert.equal(capabilities.run_mode, 'browser');
assert.equal(capabilities.mutation_enabled, false);
const readOnlyPlayer = call(readOnly, { type: 'entities' }).response.entities.find(entity => entity.name === 'player').id;
const readOnlyDetails = call(readOnly, { type: 'entity', entity: readOnlyPlayer }).response;
const readOnlyPosition = Object.keys(readOnlyDetails.components).find(name => name.endsWith('::Position'));
assert.ok(readOnlyPosition);
const readOnlyCapture = call(readOnly, { type: 'capture' }).response.checksum;
for (const operation of ['step', 'invoke', 'inject_input', 'mutate']) assert.ok(!capabilities.operations.includes(operation));
for (const request of [
  { type: 'step', frames: 1 },
  { type: 'invoke', name: 'spawn_shard', arguments: { x: 0, y: 0 } },
  { type: 'inject_input', frame: 1, actions: {} },
  { type: 'set_field', entity: readOnlyPlayer, component: readOnlyPosition, field: 'x', value: 0 },
]) {
  const result = call(readOnly, request, false);
  assert.equal(result.observed_frame, 0);
  assert.equal(result.state_revision, 0);
  assert.equal(result.error.code, 'mutation_disabled');
}
assert.equal(call(readOnly, { type: 'entities' }).response.entities.filter(entity => !entity.name?.startsWith('ui/journal/')).length, 6);
assert.equal(call(readOnly, { type: 'capture' }).response.checksum, readOnlyCapture);
assert.deepEqual(call(readOnly, { type: 'entity', entity: readOnlyPlayer }).response, readOnlyDetails);
readOnly.free();

const runtime = new BrowserRuntime(true);
const controlledCapabilities = call(runtime, { type: 'capabilities' }).response;
assert.equal(controlledCapabilities.mutation_enabled, true);
const supported = controlledCapabilities.operations;
for (const operation of ['invoke', 'step', 'inject_input', 'capture', 'mutate']) assert.ok(supported.includes(operation));
const entities = call(runtime, { type: 'entities' }).response.entities;
const shrine = entities.find(entity => entity.name === 'shrine').id;
const hud = entities.find(entity => entity.name === 'ui/quest').id;
const hudDetails = () => call(runtime, { type: 'entity', entity: hud }).response;
const uiText = Object.keys(hudDetails().components).find(name => name.endsWith('::UiText'));
assert.equal(hudDetails().components[uiText].text, 'SHARDS 0/3');
assert.equal(hudDetails().component_fields[uiText].text.writable, false);
assert.ok(call(runtime, { type: 'commands' }).response.commands.some(command => command.name === 'spawn_shard'));
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
assert.equal(call(runtime, { type: 'entities' }).response.entities.filter(entity => !entity.name?.startsWith('ui/journal/')).length, 3);
assert.equal(hudDetails().components[uiText].text, 'SHARDS 3/3  SHRINE ACTIVE');
const capture = call(runtime, { type: 'capture' }).response;
assert.equal(capture.checksum, 'f7a298f62ad75c1c');
assert.deepEqual([capture.width, capture.height], [160, 112]);
assert.ok(capture.artifact.startsWith('data:image/png;base64,'));
const png = Buffer.from(capture.artifact.split(',')[1], 'base64');
assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
const rejected = call(runtime, { type: 'invoke', name: 'spawn_shard', arguments: { x: -1, y: 0 } }, false);
assert.equal(rejected.state_revision, stepped.state_revision);
call(runtime, { type: 'invoke', name: 'spawn_shard', arguments: { x: 0, y: 0 } });
assert.notEqual(call(runtime, { type: 'capture' }).response.checksum, capture.checksum);
assert.equal(call(runtime, { type: 'status' }).observed_frame, 11);
// Field writes use the actual Rust component name discovered from this WASM build.
const player = entities.find(entity => entity.name === 'player').id;
const beforeField = call(runtime, { type: 'entity', entity: player });
const position = Object.keys(beforeField.response.components).find(name => name.endsWith('::Position'));
assert.ok(position);
assert.equal(beforeField.response.component_fields[position].x.writable, true);
assert.equal(beforeField.response.component_fields[position].x.minimum, 0);
assert.equal(beforeField.response.component_fields[position].x.maximum, 19);
assert.deepEqual(beforeField.response.component_fields[position], {
  x: { type_name: 'i32', description: 'Map tile coordinate', writable: true, minimum: 0, maximum: 19, unit: 'tile' },
  y: { type_name: 'i32', description: 'Map tile coordinate', writable: true, minimum: 0, maximum: 13, unit: 'tile' },
});
const beforeWriteCapture = call(runtime, { type: 'capture' }).response.checksum;
const written = call(runtime, { type: 'set_field', entity: player, component: position, field: 'x', value: 1 });
assert.equal(written.observed_frame, 11);
assert.equal(written.response.applied_frame, 11);
assert.equal(written.state_revision, beforeField.state_revision + 1);
const afterField = call(runtime, { type: 'entity', entity: player });
assert.equal(afterField.response.components[position].x, 1);
assert.equal(afterField.response.components[position].y, beforeField.response.components[position].y);
const afterWriteCapture = call(runtime, { type: 'capture' }).response.checksum;
assert.notEqual(afterWriteCapture, beforeWriteCapture);
for (const value of ['1', -1, 20]) {
  const invalid = call(runtime, { type: 'set_field', entity: player, component: position, field: 'x', value }, false);
  assert.equal(invalid.error.code, 'invalid_value');
  assert.equal(invalid.state_revision, written.state_revision);
  assert.equal(invalid.observed_frame, 11);
  assert.deepEqual(call(runtime, { type: 'entity', entity: player }).response, afterField.response);
  assert.equal(call(runtime, { type: 'capture' }).response.checksum, afterWriteCapture);
}
const mismatch = JSON.parse(runtime.handle(JSON.stringify({ schema_version: 999, request_id: 'mismatch', request: { type: 'status' } })));
assert.equal(mismatch.error.code, 'protocol_mismatch');
assert.equal(mismatch.request_id, 'mismatch');
runtime.free();
console.log('WASM browser control loop passed: read-only policy, replay, exact capture, command, typed field writes and rejection, schema errors.');


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

// A capture may meet its GPU/encoding budget but expire while preparing its
// final JSON. Drive the host monotonic clock at that boundary deterministically.
{
  const originalPerformance = globalThis.performance;
  let finalizing = false;
  let reads = 0;
  globalThis.performance = { now: () => finalizing && ++reads > 1 ? 6000 : 0 };
  const runtime = new BrowserRuntime(true);
  try {
    const promise = runtime.dispatch(JSON.stringify({ schema_version: 2, request_id: 'serialization-timeout', request: { type: 'capture' } }));
    finalizing = true;
    const response = JSON.parse(await promise);
    assert.equal(response.request_id, 'serialization-timeout');
    assert.equal(response.status, 'failure');
    assert.equal(response.error.code, 'timeout');
    assert.equal(response.observed_frame, 0);
  } finally {
    runtime.free();
    globalThis.performance = originalPerformance;
  }
}
