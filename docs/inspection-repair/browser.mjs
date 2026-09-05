// Run from the repository root after python3 scripts/build-browser.py.
// Actual WASM protocol/session coverage under Node; no DOM, bridge, or GPU coverage.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { encoding: 'utf8', timeout: 60000 }));
const require = createRequire(import.meta.url);
const { BrowserRuntime, BrowserLiveRuntime } = require(resolve(metadata.target_directory, 'titan/browser-node/titan_browser.js'));
const evidence = [];
function call(runtime, request_id, request, code) {
  const envelope = { schema_version: 2, request_id, request };
  const result = JSON.parse(runtime.handle(JSON.stringify(envelope)));
  assert.equal(result.request_id, request_id);
  assert.equal(result.status, code ? 'failure' : 'success');
  if (code) assert.equal(result.error.code, code);
  evidence.push({ request: envelope, result });
  return result;
}

const live = new BrowserLiveRuntime();
try {
  // Fixture explicitly opts into controls, then runs the live clock.
  live.set_control_enabled(true);
  live.resume();
  const before = call(live, 'clock-before', { type: 'status' });
  const rejected = call(live, 'clock-step', { type: 'step', frames: 1 }, 'not_controlled');
  assert.equal(rejected.observed_frame, before.observed_frame);
  assert.equal(rejected.state_revision, before.state_revision);
  assert.equal(call(live, 'clock-status', { type: 'status' }).response.paused, false);
  const capabilities = call(live, 'clock-capabilities', { type: 'capabilities' }).response;
  assert.equal(capabilities.controlled, false);
  assert.ok(!capabilities.operations.includes('step'));
  assert.ok(capabilities.operations.includes('invoke'));
  const commands = call(live, 'clock-commands', { type: 'commands' }).response.commands;
  assert.ok(commands.some(command => command.name === 'pause' && Object.keys(command.arguments ?? {}).length === 0));
  call(live, 'clock-pause', { type: 'invoke', name: 'pause', arguments: {} });
  assert.equal(call(live, 'clock-paused-status', { type: 'status' }).response.paused, true);
  assert.equal(call(live, 'clock-retry-step', { type: 'step', frames: 1 }).observed_frame, before.observed_frame + 1);
} finally { live.free(); }

const readOnly = new BrowserRuntime(false);
try {
  const before = call(readOnly, 'policy-before', { type: 'status' });
  const rejected = call(readOnly, 'policy-step', { type: 'step', frames: 1 }, 'mutation_disabled');
  assert.equal(rejected.observed_frame, before.observed_frame);
  assert.equal(rejected.state_revision, before.state_revision);
  const capabilities = call(readOnly, 'policy-capabilities', { type: 'capabilities' }).response;
  assert.equal(capabilities.mutation_enabled, false);
  assert.equal(capabilities.controlled, true);
  assert.ok(!capabilities.operations.includes('step'));
  assert.ok(!capabilities.operations.includes('invoke'));
  assert.deepEqual(call(readOnly, 'policy-commands', { type: 'commands' }).response.commands, []);
  assert.equal(call(readOnly, 'policy-status', { type: 'status' }).response.paused, true);
} finally { readOnly.free(); }

// Synchronous inspector controls create a fresh session, as the page documents.
// This is an explicit fixture authorization, never an automatic failure repair.
const optedIn = new BrowserRuntime(true);
try {
  assert.equal(call(optedIn, 'opted-in-capabilities', { type: 'capabilities' }).response.mutation_enabled, true);
  assert.equal(call(optedIn, 'opted-in-step', { type: 'step', frames: 1 }).observed_frame, 1);
} finally { optedIn.free(); }
console.log(JSON.stringify(evidence, null, 2));
