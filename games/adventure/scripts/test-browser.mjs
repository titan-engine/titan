// Run synchronous actual-WASM calls in a subprocess so a regression cannot hang CI.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from '../../../scripts/acceptance_process.mjs';

const script = fileURLToPath(import.meta.url);
const root = resolve(dirname(script), '..');
if (!process.argv.includes('--wasm-worker')) {
  const metadata = JSON.parse(await execFile('cargo', ['metadata', '--format-version', '1', '--no-deps'],
    { phase: 'build', cwd: root, encoding: 'utf8' }));
  const directory = await mkdtemp(resolve(tmpdir(), 'adventure-agreement-'));
  try {
  const trace = resolve(directory, 'native.json');
  await execFile('python3', [resolve(root, 'scripts/test-control.py'), '--trace', trace], { phase: 'build', cwd: root, encoding: 'utf8' });
  const result = await execFile(process.execPath, [script, '--wasm-worker',
    resolve(metadata.target_directory, 'titan/browser-node/titan_game.js'), trace], { encoding: 'utf8' });
  process.stdout.write(result);
  } finally { await rm(directory, { recursive: true, force: true }); }
} else {
  const { BrowserRuntime } = createRequire(import.meta.url)(process.argv[3]);
  let sequence = 0;
  const raw = (runtime, request) => JSON.parse(runtime.handle(JSON.stringify({
    schema_version: 2, request_id: `adventure-test-${++sequence}`, request,
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
  // Promise dispatch accepts before returning and keeps no player borrow while awaited.
  const dispatched = new BrowserRuntime(false);
  const requests = [
    { type: 'status' }, { type: 'capabilities' }, { type: 'capture' }, { type: 'step', frames: 1 },
  ];
  const promises = requests.map((request, index) => dispatched.dispatch(JSON.stringify({
    schema_version: 2, request_id: `dispatch-${index}`, request,
  })));
  assert.ok(promises.every(promise => typeof promise.then === 'function'));
  dispatched.free();
  const responses = (await Promise.all(promises)).map(JSON.parse);
  for (const [index, response] of responses.entries()) {
    assert.equal(response.request_id, `dispatch-${index}`);
    assert.equal(response.observed_frame, 0);
    assert.equal(response.state_revision, 0);
  }
  assert.equal(responses[0].status, 'success');
  assert.ok(!responses[1].response.operations.includes('capture'));
  assert.equal(responses[2].error.code, 'unsupported');
  assert.equal(responses[3].error.code, 'mutation_disabled');

  const readonly = new BrowserRuntime(false);
  try {
    const initial = state(readonly);
    assert.equal(ok(readonly, { type: 'capabilities' }).mutation_enabled, false);
    assert.deepEqual(ok(readonly, { type: 'commands' }).commands, []);
    for (const request of [
      { type: 'step', frames: 1 },
      { type: 'inject_input', frame: 1, actions: {} },
      invoke('restart'), invoke('switch'),
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
    ok(game, invoke('select_room', {room: 1}));
    const initial = state(game);
    const route = JSON.parse(await readFile(resolve(root, 'tests/control-route.json'), 'utf8'));
    const native = JSON.parse(await readFile(process.argv[4], 'utf8'));
    for (const [index, sample] of route.entries()) {
      const frame = ok(game, { type: 'status' }).current_frame + 1;
      ok(game, { type: 'inject_input', frame, actions: Object.fromEntries(sample.actions.map(a => [a, {kind: 'button', value: true}])) });
      ok(game, { type: 'step', frames: 1 });
      const current = state(game);
      assert.equal(current.active_character, sample.active_character);
      for (const [name, position] of Object.entries(sample.characters)) {
        for (const [axis, value] of Object.entries(position)) assert.equal(current.characters[name][axis], value, `tick ${index + 1} ${name}.${axis}`);
        assert.equal(current.characters[name].y, 0);
      }
      assert.deepEqual(current, native[index], `full native/WASM state at tick ${index + 1}`);
    }
    const expected = state(game);
    const recording = ok(game, { type: 'query', name: 'recording' }).value;
    ok(game, invoke('replay', { recording }));
    for (const key of ['characters', 'active_character', 'consumed_input', 'session_tick']) assert.deepEqual(state(game)[key], expected[key], key);
    const beforeInvalid = state(game);
    fail(game, invoke('replay', { recording: { ...recording, fixture: 'wrong' } }), 'invalid_value');
    assert.deepEqual(state(game), beforeInvalid);
    const frame = ok(game, { type: 'status' }).current_frame;
    ok(game, { type: 'inject_input', frame: frame + 1, actions: { right: { kind: 'button', value: true } } });
    ok(game, invoke('restart'));
    assert.equal(ok(game, { type: 'status' }).current_frame, frame);
    ok(game, { type: 'step', frames: 1 });
    assert.deepEqual(state(game).characters, initial.characters);
    assert.equal(state(game).active_character, 'jumper');
    ok(game, invoke('switch'));
    assert.equal(state(game).active_character, 'strong');
  } finally { game.free(); }
  for (const held of ['restart', 'jump', 'switch', 'right']) {
    const runtime = new BrowserRuntime(true);
    try {
      ok(runtime, invoke('select_room', {room: 1}));
      const injectStep = actions => {
        ok(runtime, { type: 'inject_input', frame: state(runtime).frame + 1,
          actions: Object.fromEntries(actions.map(name => [name, {kind: 'button', value: true}])) });
        ok(runtime, { type: 'step', frames: 1 });
      };
      injectStep([...new Set(['restart', held])]);
      const generation = state(runtime).session_generation;
      injectStep([held]);
      assert.equal(state(runtime).session_generation, generation, `held ${held} must not restart`);
      assert.equal(state(runtime).active_character, 'jumper', `held ${held} must not switch`);
      assert.equal(state(runtime).characters.jumper.y, 0, `held ${held} must not jump`);
      assert.equal(state(runtime).characters.jumper.x, 1500, `held ${held} must not move`);
      const expected = state(runtime);
      const recording = ok(runtime, {type: 'query', name: 'recording'}).value;
      ok(runtime, invoke('replay', {recording}));
      for (const key of ['characters', 'active_character', 'consumed_input', 'session_tick']) {
        assert.deepEqual(state(runtime)[key], expected[key], `injected restart replay ${held}: ${key}`);
      }
      const beforeFreshGeneration = state(runtime).session_generation;
      injectStep([]);
      injectStep([held]);
      const fresh = state(runtime);
      if (held === 'restart') assert.equal(fresh.session_generation, beforeFreshGeneration + 1);
      if (held === 'jump') assert.equal(fresh.characters.jumper.y, 170);
      if (held === 'switch') assert.equal(fresh.active_character, 'strong');
      if (held === 'right') assert.equal(fresh.characters.jumper.x, 1560);
    } finally { runtime.free(); }
  }
  console.log('Adventure actual-WASM: complete per-tick native agreement, held switching, replay, restart, switch command and read-only policy passed.');
}
