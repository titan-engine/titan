import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { bindKeys } from './keys.mjs';

// Execute the actual page host with controlled DOM, GPU and scheduling boundaries.
const source = (await readFile(new URL('./play.js', import.meta.url), 'utf8'))
  .replace("import { bindKeys } from './keys.mjs';", '')
  .replace("await import('../inspector/pkg/titan_game.js')", 'await loadModule()');
class Events {
  handlers = {};
  addEventListener(type, callback) { (this.handlers[type] ??= []).push(callback); }
  emit(type, event = {}) { for (const callback of this.handlers[type] ?? []) callback({ preventDefault() {}, ...event }); }
}
async function page({ hidden = false, focused = true, fail = false, delayed = false } = {}) {
  const window = new Events(), document = new Events(), elements = new Map();
  document.hidden = hidden;
  document.hasFocus = () => focused;
  document.getElementById = id => {
    if (!elements.has(id)) elements.set(id, { disabled: id !== 'play', checked: false, textContent: '' });
    return elements.get(id);
  };
  const canvas = { focus() { document.activeElement = canvas; }, getBoundingClientRect: () => ({ width: 960, height: 540 }) };
  document.querySelector = () => canvas;
  let paused = true, ticks = 0, cleared = 0, reloads = 0, ready;
  const frames = [], keys = [];
  const player = {
    pause() { paused = true; }, resume() { paused = false; }, paused: () => paused,
    status: () => JSON.stringify({ paused, ticks }), set_control_enabled(value) { this.control = value; },
    resize() {}, frame() { if (!paused) ticks++; }, clear_input() { cleared++; }, set_key(...args) { keys.push(args); },
    step() { if (!paused) throw Error('Pause first'); ticks++; }, restart() { ticks = 0; },
    load_recording() { paused = true; },
    async dispatch(json) {
      if (!this.control) return JSON.stringify({ status: 'error' });
      if (JSON.parse(json).request.type === 'invoke') paused = true;
      return JSON.stringify({ status: 'success' });
    },
  };
  const context = {
    window, document, bindKeys: options => bindKeys({ ...options, window, document }),
    location: { href: 'http://localhost/play/', reload() { reloads++; } },
    URL, devicePixelRatio: 1, ResizeObserver: class { observe() {} },
    setTimeout, clearTimeout, requestAnimationFrame: callback => frames.push(callback),
    loadModule: async () => ({ default: async () => {}, BrowserPlayer: { create: async () => {
      if (delayed) await new Promise(resolve => { ready = resolve; });
      if (fail) throw Error('adapter unavailable');
      return player;
    } } }),
  };
  vm.runInNewContext(source, context);
  const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
  await settle();
  return { window, document, canvas, player, keys, elements, settle, get paused() { return paused; },
    get ticks() { return ticks; }, get cleared() { return cleared; }, get reloads() { return reloads; },
    ready: async () => { ready(); await settle(); },
    focus(value) { focused = value; window.emit(value ? 'focus' : 'blur'); },
    visibility(value) { document.hidden = value; document.emit('visibilitychange'); },
    frame() { frames.shift()?.(16); },
  };
}
test('visible focused page starts without a click and immediately accepts movement', async () => {
  const p = await page();
  assert.equal(p.paused, false);
  assert.equal(p.document.activeElement, p.canvas);
  assert.equal(p.elements.get('play').hidden, true);
  assert.equal(p.elements.get('pause').textContent, 'Pause');
  assert.equal(p.elements.get('step').disabled, true);
  assert.equal(p.player.control, false);
  p.window.emit('keydown', { target: p.canvas, code: 'KeyD', repeat: false });
  assert.deepEqual(p.keys, [['KeyD', true, false]]);
  p.frame(); assert.equal(p.ticks, 1);
});
test('loading waits for both visibility and focus, including a loss during initialization', async () => {
  const p = await page({ delayed: true });
  assert.match(p.elements.get('status').textContent, /Loading/);
  assert.equal(p.elements.get('pause').disabled, true);
  p.focus(false); p.visibility(true); await p.ready();
  assert.equal(p.paused, true);
  p.focus(true); assert.equal(p.paused, true);
  p.visibility(false); assert.equal(p.paused, false);
  p.visibility(true); assert.equal(p.paused, true);
  p.visibility(false); assert.equal(p.paused, true);
  assert.equal(p.elements.get('pause').textContent, 'Resume');
});
test('manual pause, step, later blur and imported recordings never auto-resume', async () => {
  const p = await page();
  p.elements.get('pause').onclick(); assert.equal(p.paused, true);
  p.elements.get('step').onclick(); assert.equal(p.ticks, 1);
  p.focus(false); p.focus(true); assert.equal(p.paused, true);
  p.elements.get('pause').onclick(); assert.equal(p.paused, false);
  p.focus(false); p.focus(true); assert.equal(p.paused, true);
  assert.ok(p.cleared > 0);
  const q = await page({ focused: false });
  await q.elements.get('import').onchange({ target: { files: [{ size: 2, text: async () => '{}' }] } });
  q.focus(true); assert.equal(q.paused, true);
});
test('successful inspector pause before first focus cancels startup; rejected writes do not', async () => {
  for (const allowed of [false, true]) {
    const p = await page({ focused: false });
    p.elements.get('control').checked = allowed; p.elements.get('control').onchange();
    await p.window.collectionRoom.dispatch(JSON.stringify({ request: { type: 'invoke', command: 'pause' } }));
    p.focus(true); assert.equal(p.paused, allowed);
  }
});
test('initialization failure shows error, disables controls and exposes a fresh-session retry', async () => {
  const p = await page({ fail: true });
  assert.match(p.elements.get('error').textContent, /adapter unavailable/);
  assert.match(p.elements.get('status').textContent, /Error/);
  assert.equal(p.elements.get('play').hidden, false);
  assert.equal(p.elements.get('play').disabled, false);
  assert.equal(p.elements.get('pause').disabled, true);
  p.elements.get('play').onclick(); assert.equal(p.reloads, 1);
  p.focus(true); assert.equal(p.paused, true);
});

test('a graphics failure stops scheduling playable frames and keeps error controls accurate', async () => {
  const p = await page();
  p.player.frame = () => { throw Error('device lost'); };
  p.frame();
  assert.equal(p.paused, true);
  assert.match(p.elements.get('error').textContent, /device lost/);
  assert.equal(p.elements.get('step').disabled, true);
  p.focus(false); p.focus(true); p.frame();
  assert.match(p.elements.get('status').textContent, /Error/);
  assert.equal(p.elements.get('pause').disabled, true);
});
