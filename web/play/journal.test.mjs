import assert from 'node:assert/strict';
import test from 'node:test';
import { bindJournalInput } from './journal.mjs';

function setup() {
  const surface = () => ({ handlers: {}, addEventListener(type, fn) { this.handlers[type] = fn; } });
  const canvas = Object.assign(surface(), { width: 800, height: 560, focus() {}, setPointerCapture() {}, getBoundingClientRect: () => ({ left: 10, top: 20, width: 400, height: 280 }) });
  const window = surface(), document = surface();
  let open = false, cancellations = 0;
  const keys = [], points = [];
  const session = { journal_open: () => open, journal_key(key) { keys.push(key); if (key === 'toggle') open = !open; return open || key === 'close'; }, journal_pointer(...point) { points.push(point); return true; }, cancel_journal_input() { cancellations++; } };
  const binding = bindJournalInput({ canvas, player: () => session, changed() {}, window, document });
  const event = extra => ({ pointerId: 1, button: 0, clientX: 210, clientY: 160, preventDefault() {}, ...extra });
  return { canvas, window, document, binding, event, keys, points, cancellations: () => cancellations };
}

test('journal keyboard consumes modal movement and repeats while preserving native focus navigation', () => {
  const h = setup();
  assert.equal(h.binding.onKey(h.event({ key: 'ArrowUp' })), false);
  assert.equal(h.binding.onKey(h.event({ key: 'j' })), true);
  assert.equal(h.binding.onKey(h.event({ key: 'Tab', shiftKey: true })), true);
  assert.equal(h.binding.onKey(h.event({ key: 'w' })), true);
  assert.equal(h.binding.onKey(h.event({ key: 'j', repeat: true })), true);
  assert.deepEqual(h.keys, ['previous', 'toggle', 'previous']);
  h.binding.onKey(h.event({ key: 'j' }));
  assert.equal(h.binding.onKey(h.event({ key: 'w', repeat: true })), true);
  assert.equal(h.binding.onKey(h.event({ key: 'w', repeat: false })), false);
});

test('journal pointer scales backing pixels and rejects interrupted or competing releases', () => {
  const h = setup();
  h.canvas.handlers.pointerdown(h.event());
  assert.deepEqual(h.points, [[400, 280, true]]);
  h.canvas.handlers.pointerup(h.event({ pointerId: 2 }));
  assert.equal(h.points.length, 1);
  h.canvas.handlers.pointercancel(h.event());
  h.canvas.handlers.pointerup(h.event());
  assert.equal(h.points.length, 1);
  assert.equal(h.cancellations(), 1);
  h.canvas.handlers.pointerdown(h.event());
  h.canvas.handlers.pointerup(h.event({ clientX: 610 }));
  assert.deepEqual(h.points.at(-1), [1200, 280, false]);
  h.canvas.handlers.lostpointercapture(h.event());
  assert.equal(h.cancellations(), 1);
});

test('epoch, focus loss and document hiding cancel outstanding physical gestures', () => {
  const h = setup();
  h.canvas.handlers.pointerdown(h.event());
  h.binding.cancelHeld();
  h.canvas.handlers.pointerup(h.event());
  assert.equal(h.points.length, 1);
  h.window.handlers.blur();
  h.document.handlers.focusin({ target: {} });
  h.document.hidden = true;
  h.document.handlers.visibilitychange();
  assert.equal(h.cancellations(), 3);
});

test('journal transitions clear shared held movement and a held key cannot resume through repeat', async () => {
  const { bindPlayerInput } = await import('../shared/input.mjs');
  const surface = () => ({ handlers: {}, addEventListener(type, callback) { (this.handlers[type] ??= []).push(callback); }, emit(type, event) { for (const callback of this.handlers[type] ?? []) callback(event); } });
  const canvas = surface(), window = surface(), document = surface();
  let open = false, moving = false;
  const player = { journal_open: () => open, journal_key(key) { if (key !== 'toggle') return open; open = !open; return true; }, cancel_journal_input() {} };
  let input;
  const journal = bindJournalInput({ canvas, window, document, player: () => player, changed: () => input.cancel() });
  input = bindPlayerInput({ canvas, window, document, buttons: [], keys: new Map([['w', 'up']]), actions: ['up'], isRunning: () => !open,
    setAction: (_action, pressed) => { moving = pressed; }, cancelAction() {}, clearInput: () => { moving = false; }, onKey: event => journal.onKey(event) });
  const key = (key, code, repeat = false) => window.emit('keydown', { key, code, repeat, target: canvas, preventDefault() {} });
  key('w', 'KeyW'); assert.equal(moving, true);
  key('j', 'KeyJ'); assert.equal(open, true); assert.equal(moving, false);
  key('j', 'KeyJ'); assert.equal(open, false); assert.equal(moving, false);
  key('w', 'KeyW', true); assert.equal(moving, false);
  window.emit('keyup', { code: 'KeyW', preventDefault() {} });
  key('w', 'KeyW'); assert.equal(moving, true);
  window.emit('blur'); assert.equal(moving, false);
  key('w', 'KeyW', true); assert.equal(moving, false);
});
