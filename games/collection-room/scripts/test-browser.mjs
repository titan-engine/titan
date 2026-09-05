// Run synchronous actual-WASM calls in a subprocess so a regression cannot hang CI.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from '../../../scripts/acceptance_process.mjs';

const script = fileURLToPath(import.meta.url);
const root = resolve(dirname(script), '..');
if (!process.argv.includes('--wasm-worker')) {
  const metadata = JSON.parse(await execFile('cargo', ['metadata', '--format-version', '1', '--no-deps'],
    { phase: 'build', cwd: root, encoding: 'utf8' }));
  const result = await execFile(process.execPath, [script, '--wasm-worker',
    resolve(metadata.target_directory, 'titan/browser-node/titan_game.js')], { encoding: 'utf8' });
  process.stdout.write(result);
} else {
  const { BrowserRuntime } = createRequire(import.meta.url)(process.argv[3]);
  let sequence = 0;
  const raw = (runtime, request) => JSON.parse(runtime.handle(JSON.stringify({
    schema_version: 1, request_id: `collection-test-${++sequence}`, request,
  })));
  function ok(runtime, request) {
    const response = raw(runtime, request);
    assert.equal(response.status, 'success', JSON.stringify(response));
    return response.response;
  }
  function fail(runtime, request, code) {
    const response = raw(runtime, request);
    assert.equal(response.status, 'failure', JSON.stringify(response));
    assert.equal(response.error.code, code);
    return response;
  }
  const invoke = (name, args = {}) => ({ type: 'invoke', name, arguments: args });
  const state = runtime => ok(runtime, { type: 'query', name: 'state' }).value;
  const readonly = new BrowserRuntime(false);
  try {
    const initial = state(readonly);
    assert.equal(ok(readonly, { type: 'capabilities' }).mutation_enabled, false);
    assert.deepEqual(ok(readonly, { type: 'commands' }).commands, []);
    for (const request of [
      { type: 'step', frames: 1 },
      { type: 'inject_input', frame: 1, actions: {} },
      invoke('restart'), invoke('teleport', { x: -2000, z: 3000 }),
      { type: 'set_field', entity: { index: 0, generation: 0 }, component: 'Position', field: 'x', value: 0 },
    ]) {
      const before = raw(readonly, { type: 'status' });
      const rejected = fail(readonly, request, 'mutation_disabled');
      assert.equal(rejected.observed_frame, before.observed_frame);
      assert.equal(rejected.state_revision, before.state_revision);
      assert.deepEqual(state(readonly), initial);
    }
  } finally { readonly.free(); }

  const game = new BrowserRuntime(true);
  try {
    assert.equal(ok(game, { type: 'capabilities' }).mutation_enabled, true);
    const entities = ok(game, { type: 'entities', query: {}, page: { limit: 100 } }).entities;
    const names = entities.map(entity => entity.name);
    assert.equal(new Set(names).size, names.length);
    const player = entities.find(entity => entity.name === 'player');
    assert.ok(player);
    const details = ok(game, { type: 'entity', entity: player.id });
    for (const name of ['floor', 'obstacle-1', 'obstacle-2', 'collectible-1', 'collectible-2', 'collectible-3']) assert.ok(names.includes(name), name);
    const positionKey = Object.keys(details.components).find(key => key.endsWith('::Position'));
    assert.deepEqual(details.components[positionKey], { x: -3000, z: 3000 });
    for (const axis of ['x', 'z']) assert.equal(details.component_fields[positionKey][axis].writable, false);
    const progressKey = Object.keys(details.components).find(key => key.endsWith('::Progress'));
    assert.equal(details.components[progressKey].collected, 0);
    const initial = state(game);
    assert.deepEqual(initial.position, { x: -3000, z: 3000 });
    assert.equal(initial.collected, 0);
    assert.equal(initial.total, 3);
    assert.equal(initial.completed, false);
    for (const position of [{ x: 5000, z: 0 }, { x: 0, z: 0 }, { x: 'bad', z: 0 }]) {
      const before = raw(game, { type: 'status' });
      const rejected = fail(game, invoke('teleport', position), 'invalid_value');
      assert.equal(rejected.observed_frame, before.observed_frame);
      assert.equal(rejected.state_revision, before.state_revision);
      assert.deepEqual(state(game), initial);
    }
    function drive(actions) {
      const frame = ok(game, { type: 'status' }).current_frame;
      for (const [index, action] of actions.entries()) {
        ok(game, { type: 'inject_input', frame: frame + index + 1,
          actions: { [action]: { kind: 'button', value: true } } });
      }
      ok(game, { type: 'step', frames: actions.length });
    }
    ok(game, invoke('teleport', { x: -3000, z: 0 }));
    drive(Array(20).fill('right'));
    assert.ok(state(game).position.x < -750, 'central obstacle blocks movement');
    ok(game, invoke('restart'));
    drive([...Array(8).fill('right'), ...Array(20).fill('up'), ...Array(16).fill('right')]);
    const won = state(game);
    assert.deepEqual(won.position, { x: 3000, z: -2000 });
    assert.equal(won.collected, 3);
    assert.equal(won.completed, true);
    assert.deepEqual(won.remaining, []);
    const recording = ok(game, { type: 'query', name: 'recording' }).value;
    assert.equal(recording.frames.length, 44);
    assert.equal(recording.truncated, false);
    ok(game, { type: 'step', frames: 4 });
    assert.equal(state(game).collected, 3, 'collection cannot repeat');
    ok(game, invoke('replay', { recording }));
    const replayed = state(game);
    for (const key of ['position', 'collected', 'total', 'completed', 'remaining', 'session_tick']) {
      assert.deepEqual(replayed[key], won[key], `replay ${key}`);
    }
    const beforeInvalid = state(game);
    fail(game, invoke('replay', { recording: { ...recording, fixture: 'wrong' } }), 'invalid_value');
    assert.deepEqual(state(game), beforeInvalid, 'invalid replay is transactional');
    const frame = ok(game, { type: 'status' }).current_frame;
    ok(game, { type: 'inject_input', frame: frame + 1, actions: { right: { kind: 'button', value: true } } });
    ok(game, invoke('restart'));
    assert.equal(ok(game, { type: 'status' }).current_frame, frame);
    ok(game, { type: 'step', frames: 1 });
    const restarted = state(game);
    assert.deepEqual(restarted.position, initial.position, 'restart clears pending inputs');
    assert.equal(restarted.collected, 0);
    assert.equal(restarted.completed, false);
    assert.deepEqual(restarted.remaining, initial.remaining);
  } finally { game.free(); }
  console.log('Collection room actual-WASM: policy, named fields, transactional teleport/replay, blocked movement, 44-tick win/replay and restart passed.');
}
