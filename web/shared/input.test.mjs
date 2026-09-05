import { bindCanvasPointer } from '../../games/arena/web/play/pointer.mjs';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import { bindPlayerInput } from './input.mjs';
import { bindJournalInput } from '../play/journal.mjs';

// Exercise each shipped page's lifecycle with event-buffering semantics. The
// native and actual-WASM tests separately verify game-specific tick sampling.
for (const page of ['../play/play.js', '../../starters/minimal/web/play/play.js', '../../games/arena/web/play/play.js']) {
  test(`${page}: releases retain taps, cancellation discards only affected input`, async () => {
    function surface() {
      const handlers = {};
      return {
        handlers, clientWidth: 640, clientHeight: 448,
        addEventListener(name, fn) {
          const previous = handlers[name];
          handlers[name] = previous ? event => { previous(event); fn(event); } : fn;
        },
        closest() { return null; },
        focus() { document.handlers.focusin?.({ target: this }); },
        setPointerCapture() {},
        click() { return handlers.click?.(); },
      };
    }
    const ids = Object.fromEntries(['game', 'start', 'pause', 'restart', 'replay', 'status', 'result', 'error', 'inspect', 'capture', 'recording', 'enable-controls', 'step', 'live-mode', 'live-output', 'live-summary', 'live-capture', 'save', 'load-save', 'load-recording', 'restart-playback', 'exit-playback', 'playback-status', 'seek-position', 'seek-playback', 'playback-speed', 'recording-result'].map(id => [id, surface()]));
    const buttons = ['up', 'down', 'left', 'right', 'dash'].map(action => Object.assign(surface(), { dataset: { action } }));
    const window = surface();
    const document = Object.assign(surface(), {
      querySelector: selector => ids[selector.slice(1)],
      querySelectorAll: () => buttons,
      hidden: false,
    });
    const held = new Set();
    const pending = new Set();
    let paused = true;
    let epoch = 0;
    const player = {
      set_action(action, pressed) {
        if (pressed) { if (!held.has(action)) pending.add(action); held.add(action); }
        else held.delete(action);
      },
      clear_input() { held.clear(); pending.clear(); },
      cancel_action(action) { held.delete(action); pending.delete(action); },
      pointer() { return false; }, cancel_pointer() {},
      journal_open() { return false; }, journal_key() { return false; },
      journal_pointer() { return false; }, cancel_journal_input() {},
      resize() {}, frame() {}, free() {}, replay_reference() {},
      restart() { paused = true; epoch++; },
      pause() { paused = true; epoch++; },
      resume() { paused = false; epoch++; },
      paused: () => paused,
      clock_epoch: () => String(epoch),
      control_enabled: () => false,
      playback_active: () => false,
      playback_status: () => JSON.stringify({active:false}),
      status: () => JSON.stringify({ frame: 0, run: { health: 3, elapsed: 0, outcome: 'Running', dash_ready: true } }),
    };
    const context = vm.createContext({
      window, document, URLSearchParams, location: { search: '' },
      requestAnimationFrame: () => 1, cancelAnimationFrame() {},
      ResizeObserver: class { observe() {} },
      bindJournalInput: options => bindJournalInput({ ...options, window, document }),
      bindCanvasPointer: options => bindCanvasPointer({ ...options, window, document }),
      bindPlayerInput: options => bindPlayerInput({ ...options, window, document }),
      init: async () => {}, BrowserPlayer: { create: async () => player, create_with_pngs: async () => player },
      loadRpgPngs: async () => ({ player: new Uint8Array(), tree: new Uint8Array() }),
    });
    vm.runInContext(readFileSync(new URL(page, import.meta.url), 'utf8').replace(/^import.*\n/gm, ''), context);
    const key = (type, value = 'w', code = 'KeyW', target = ids.game) => window.handlers[type]({ key: value, code, target, preventDefault() {} });
    const pointer = (type, action, pointerId = 1) => buttons.find(button => button.dataset.action === action).handlers[type]({ pointerId, preventDefault() {} });
    const tap = () => { key('keydown'); key('keyup'); };
    await ids.start.click();

    tap();
    assert.equal(pending.has('up'), true, 'ordinary keyup retains buffered tap');
    window.handlers.blur();
    assert.equal(pending.size, 0, 'blur cancels buffered tap');
    tap();
    ids.pause.click();
    assert.equal(pending.size, 0, 'pause cancels buffered tap');
    key('keydown');
    assert.equal(held.size, 0, 'pause ignores new input');
    ids.pause.click();
    tap();
    document.hidden = true;
    document.handlers.visibilitychange();
    assert.equal(pending.size, 0, 'hidden document cancels buffered tap');
    document.hidden = false;
    tap();
    document.handlers.focusin({ target: ids.pause });
    assert.equal(pending.size, 0, 'focus transfer cancels buffered tap');
    key('keydown', 'w', 'KeyW', { closest: () => true });
    assert.equal(held.size, 0, 'editing controls keep their keyboard input');

    key('keydown');
    key('keydown', 'ArrowUp', 'ArrowUp');
    key('keyup');
    assert.equal(held.has('up'), true, 'releasing one alias preserves another');
    pointer('pointerdown', 'up');
    pointer('pointercancel', 'up');
    assert.equal(held.has('up'), true, 'pointer cancellation preserves keyboard alias');
    window.handlers.blur();

    // Distinct queued action must survive cancellation of this gesture.
    tap();
    pointer('pointerdown', 'right');
    pointer('pointercancel', 'right');
    assert.deepEqual([...pending], ['up']);
    pointer('pointerdown', 'right');
    pointer('lostpointercapture', 'right');
    assert.deepEqual([...pending], ['up']);
    pointer('pointerdown', 'right');
    pointer('pointerup', 'right');
    pointer('lostpointercapture', 'right');
    assert.deepEqual([...pending], ['up', 'right'], 'capture loss after release preserves tap');
    window.handlers.pagehide();
    assert.equal(pending.size, 0, 'page disposal cancels pending input');
  });
}
