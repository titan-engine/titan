import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

for (const failedSprite of ['player.png', 'tree.png']) test('RPG page uses authoritative pause and exposes local playback without inspection opt-in', async () => {
  const element = () => ({
    handlers: {}, clientWidth: 640, clientHeight: 448,
    addEventListener(name, callback) { this.handlers[name] = callback; },
    focus() {},
  });
  const ids = Object.fromEntries(['game', 'start', 'pause', 'replay', 'status', 'result', 'error', 'load-recording', 'recording', 'step', 'restart-playback', 'exit-playback', 'playback-status', 'recording-result', 'inspect', 'enable-controls', 'live-output'].map(id => [id, element()]));
  const buttons = [element(), element()];
  let paused = true;
  let epoch = 0;
  let active = false;
  let position = 0;
  let actionCalls = 0;
  const player = {
    resize() {}, frame() {}, free() {}, clear_input() {},
    paused: () => paused,
    journal_open: () => false,
    clock_epoch: () => String(epoch),
    playback_active: () => active,
    playback_status: () => JSON.stringify({ active, position, total: 2, complete: active && position === 2, verified: active && position === 2 }),
    status: () => JSON.stringify({ frame: position, collected_shards: position, shrine_active: false }),
    set_action() { actionCalls++; },
    pause() { paused = true; epoch++; },
    resume() { paused = false; epoch++; },
    load_recording() { active = true; position = 0; epoch++; },
    step_playback() { position++; },
    restart_playback() { position = 0; epoch++; },
    exit_playback() { active = false; position = 0; epoch++; },
  };
  let input;
  let failAsset = true;
  let created = 0;
  const window = element();
  const context = vm.createContext({
    document: { querySelector: selector => ids[selector.slice(1)], querySelectorAll: () => buttons },
    window, location: { origin: 'http://localhost' },
    ResizeObserver: class { observe() {} },
    requestAnimationFrame: () => 1, cancelAnimationFrame() {},
    init: async () => {}, BrowserPlayer: { create_with_pngs: async (_canvas, playerBytes, treeBytes) => { assert.deepEqual([...playerBytes], [1]); assert.deepEqual([...treeBytes], [2]); created++; return player; } },
    loadRpgPngs: async () => { if (failAsset) throw new Error(`Could not load ../assets/${failedSprite}: HTTP 404`); return { player: new Uint8Array([1]), tree: new Uint8Array([2]) }; },
    bindJournalInput: () => ({ cancel() {}, cancelHeld() {}, onKey: () => false }),
    bindPlayerInput: options => { input = options; return { cancel() {} }; },
    readRecordingForSession: async file => { if (file.reject) throw new Error('Invalid recording'); return '{}'; },
  });
  vm.runInContext(readFileSync(new URL('./play.js', import.meta.url), 'utf8').replace(/^import.*\n/gm, ''), context);
  const click = id => ids[id].handlers.click();
  await click('start');
  assert.equal(created, 0, 'failed asset must not construct or run a game');
  assert.equal(paused, true);
  assert.match(ids.error.textContent, new RegExp(`${failedSprite}: HTTP 404`));
  assert.equal(ids.start.textContent, 'Retry');
  assert.equal(ids.start.disabled, false);
  failAsset = false;
  await click('start');
  assert.equal(created, 1);
  assert.equal(ids.error.hidden, true);
  assert.equal(paused, false);
  assert.equal(input.isRunning(), true);
  click('pause');
  assert.equal(paused, true);
  ids['load-recording'].files = [{ size: 2, reject: true }];
  await ids['load-recording'].handlers.change();
  assert.match(ids['recording-result'].textContent, /Load failed: Invalid recording/);
  assert.equal(active, false);
  ids['load-recording'].files = [{ size: 2 }];
  await ids['load-recording'].handlers.change();
  assert.equal(active, true);
  assert.equal(ids.step.disabled, false);
  assert.equal(ids['enable-controls'].checked, false);
  assert.equal(input.isRunning(), false);
  assert.equal(actionCalls, 0);
  click('step');
  assert.match(ids['playback-status'].textContent, /1\/2/);
  click('step');
  assert.match(ids['playback-status'].textContent, /Complete · MATCH/);
  assert.equal(ids.pause.disabled, true);
  assert.equal(ids.step.disabled, true);
  click('restart-playback');
  assert.equal(position, 0);
  assert.equal(paused, true);
  assert.equal(ids.step.disabled, false);
  ids['live-output'].textContent = 'stale inspected replay state';
  ids['recording-result'].textContent = 'Recording verified and loaded';
  click('exit-playback');
  assert.equal(ids['live-output'].textContent, '');
  assert.equal(ids['recording-result'].textContent, '');
  assert.equal(active, false);
  assert.equal(paused, true);
  assert.equal(ids.replay.disabled, false);
});
