import assert from 'node:assert/strict';
import test from 'node:test';
import { bindCanvasPointer, canvasPoint } from './pointer.mjs';

function fixture() {
  const surface = () => ({ handlers: {}, addEventListener(type, handler) { this.handlers[type] = handler; } });
  const window = surface();
  const document = surface();
  const captures = new Set();
  let cancelled = 0;
  let enabled = true;
  const calls = [];
  const canvas = Object.assign(surface(), {
    getBoundingClientRect: () => ({ left: 20, top: 30, width: 800, height: 560 }),
    focus() {},
    setPointerCapture: id => captures.add(id),
    hasPointerCapture: id => captures.has(id),
    releasePointerCapture: id => captures.delete(id),
  });
  const binding = bindCanvasPointer({ canvas, window, document,
    enabled: () => enabled,
    pointer: (x, y, pressed) => { calls.push([x, y, pressed]); return true; },
    cancelPointer: () => { cancelled++; },
  });
  function fire(target, type, changes = {}) {
    const event = { pointerId: 1, button: 0, isPrimary: true, clientX: 620, clientY: 550,
      prevented: false, stopped: false,
      preventDefault() { this.prevented = true; }, stopPropagation() { this.stopped = true; }, ...changes };
    target.handlers[type](event);
    return event;
  }
  return { canvas, window, document, captures, calls, binding, fire,
    cancelled: () => cancelled, disable: () => { enabled = false; } };
}

test('CSS surface mapping handles scaled canvas and rejects outside/invalid points', () => {
  const { canvas } = fixture();
  assert.deepEqual(canvasPoint(canvas, { clientX: 620, clientY: 550 }), [120, 104]);
  assert.equal(canvasPoint(canvas, { clientX: 820, clientY: 550 }), null);
  assert.equal(canvasPoint(canvas, { clientX: 19, clientY: 30 }), null);
  assert.equal(canvasPoint(canvas, { clientX: NaN, clientY: 30 }), null);
});

test('UI clicks route while paused, consume both edges and do not double-release', () => {
  const f = fixture(); // Enabled means a session exists; playback mode is irrelevant.
  const down = f.fire(f.canvas, 'pointerdown');
  assert.equal(down.prevented && down.stopped, true);
  assert.equal(f.captures.has(1), true);
  const up = f.fire(f.canvas, 'pointerup');
  assert.equal(up.prevented && up.stopped, true);
  f.fire(f.window, 'pointerup');
  f.fire(f.canvas, 'lostpointercapture');
  assert.deepEqual(f.calls, [[120, 104, true], [120, 104, false]]);
  assert.equal(f.captures.size, 0);
  assert.equal(f.cancelled(), 1, 'fresh local press discards any previous pending gesture');
});

test('release outside, cancellation, focus loss and resize cancellation drop orphan releases', () => {
  for (const cancellation of ['outside', 'pointercancel', 'lostpointercapture', 'blur', 'focus', 'hidden', 'resize']) {
    const f = fixture();
    f.fire(f.canvas, 'pointerdown');
    if (cancellation === 'outside') f.fire(f.canvas, 'pointerup', { clientX: 900 });
    else if (cancellation === 'blur') f.fire(f.window, 'blur');
    else if (cancellation === 'focus') f.fire(f.document, 'focusin', { target: {} });
    else if (cancellation === 'hidden') { f.document.hidden = true; f.fire(f.document, 'visibilitychange'); }
    else if (cancellation === 'resize') f.binding.cancel();
    else f.fire(f.canvas, cancellation);
    f.fire(f.canvas, 'pointerup');
    assert.deepEqual(f.calls, [[120, 104, true]], cancellation);
    assert.equal(f.captures.size, 0, cancellation);
    assert.equal(f.cancelled(), 2, cancellation);
  }
});

test('secondary pointers and unavailable sessions do not interfere', () => {
  const f = fixture();
  f.fire(f.canvas, 'pointermove');
  f.fire(f.canvas, 'pointerup');
  assert.equal(f.calls.length, 0, 'hover and orphan releases cannot complete a remote gesture');
  f.fire(f.canvas, 'pointerdown', { button: 2 });
  f.fire(f.canvas, 'pointerdown', { isPrimary: false });
  assert.equal(f.calls.length, 0);
  f.fire(f.canvas, 'pointerdown');
  f.fire(f.canvas, 'pointerdown', { pointerId: 2 });
  f.fire(f.canvas, 'pointerup', { pointerId: 2 });
  assert.equal(f.calls.length, 1);
  f.binding.cancel(); f.disable();
  f.fire(f.canvas, 'pointerdown');
  assert.equal(f.calls.length, 1);
});
