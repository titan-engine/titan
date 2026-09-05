import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import { bindPlayerInput } from '../shared/input.mjs';

// Exercise the shipped handlers with a buffered-input double. Actual WASM
// game semantics are covered separately by scripts/test-browser.mjs.
test('dash input preserves taps and cancels interrupted gestures', async () => {
  function surface() {
    const handlers = {};
    return {
      handlers, disabled: false, textContent: '', clientWidth: 640, clientHeight: 448,
      addEventListener(name, fn) { handlers[name] = fn; },
      closest() { return null; },
      focus() { document.handlers.focusin?.({ target: this }); },
      setPointerCapture() {},
      click() { return handlers.click?.(); },
    };
  }
  const ids = Object.fromEntries(['game', 'start', 'pause', 'restart', 'status', 'result', 'error'].map(key => [key, surface()]));
  const buttons = ['up', 'down', 'left', 'right', 'dash'].map(action => Object.assign(surface(), { dataset: { action } }));
  const window = surface();
  const document = Object.assign(surface(), {
    querySelector: selector => ids[selector.slice(1)],
    querySelectorAll: () => buttons,
    hidden: false,
  });
  const held = {};
  let restartCount = 0;
  let pending = false;
  const player = {
    clear_input() {
      pending = false;
      for (const action in held) held[action] = false;
    },
    cancel_action(action) {
      held[action] = false;
      if (action === 'dash') pending = false;
    },
    set_action(action, pressed) {
      if (action === 'dash' && pressed && !held[action]) pending = true;
      held[action] = pressed;
    },
    resize() {}, frame() {}, free() {},
    status: () => JSON.stringify({ run: { health: 3, elapsed: 0, outcome: 'Running', dash_ready: true, dash_cooldown: 0 } }),
    restart() { restartCount++; },
  };
  const context = {
    window, document, URLSearchParams, location: { search: '' },
    requestAnimationFrame: () => 1, cancelAnimationFrame() {},
    ResizeObserver: class { observe() {} },
    bindPlayerInput: options => bindPlayerInput({ ...options, window, document }),
    init: async () => {}, BrowserPlayer: { create: async () => player },
  };
  vm.createContext(context);
  vm.runInContext(readFileSync(new URL('./play.js', import.meta.url), 'utf8').replace(/^import.*\n/gm, ''), context);
  const key = (type, value = ' ', code = 'Space') => window.handlers[type]({ key: value, code, target: ids.game, preventDefault() {} });
  const dash = buttons.at(-1);
  const pointer = (type, pointerId) => dash.handlers[type]({ pointerId, preventDefault() {} });

  await ids.start.click();
  key('keydown');
  assert.equal(held.dash, true);
  key('keydown'); // Browser repeat remains a held press.
  assert.equal(held.dash, true);
  key('keyup');
  assert.equal(held.dash, false);
  assert.equal(pending, true, 'keyup retains a short tap');
  window.handlers.blur();
  assert.equal(pending, false);

  key('keydown');
  ids.pause.click();
  assert.equal(held.dash, false);
  assert.equal(pending, false);
  key('keydown');
  assert.equal(held.dash, false, 'paused player ignores new keys');
  ids.pause.click();
  key('keydown');
  document.handlers.focusin({ target: ids.pause });
  assert.equal(pending, false);
  key('keydown');
  document.hidden = true;
  document.handlers.visibilitychange();
  assert.equal(pending, false);

  let defaultPrevented = false;
  dash.handlers.pointerdown({ pointerId: 1, preventDefault() { defaultPrevented = true; } });
  assert.equal(defaultPrevented, true, 'prevent button focus from canceling pointerdown');
  assert.equal(pending, true);
  pointer('pointercancel', 1);
  assert.equal(pending, false, 'canceled pointer drops pending tap');
  pointer('pointerdown', 2);
  pointer('lostpointercapture', 2);
  assert.equal(pending, false, 'unexpected capture loss drops pending tap');
  pointer('pointerdown', 3);
  pointer('pointerup', 3);
  pointer('lostpointercapture', 3);
  assert.equal(pending, true, 'normal pointerup and capture loss retain tap');
  window.handlers.blur();

  key('keydown');
  pointer('pointerdown', 4);
  pointer('pointercancel', 4);
  assert.equal(held.dash, true, 'canceling pointer preserves keyboard dash');
  assert.equal(pending, true);
  key('keyup');
  ids.restart.click();
  assert.equal(held.dash, false);
  assert.equal(pending, false);
  assert.equal(restartCount, 1);
  assert.equal(ids.pause.textContent, 'Resume');
  assert.match(ids.status.textContent, /Dash ready/);
});
