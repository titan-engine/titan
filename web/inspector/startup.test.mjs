import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';

test('inspector asset failure clears stale inspection controls and retry restores a fresh session', async () => {
  const element = () => ({ handlers: {}, children: [], hidden: false, textContent: '', value: '0', dataset: {},
    addEventListener(type, callback) { this.handlers[type] = callback; },
    replaceChildren(...children) { this.children = children; }, append(...children) { this.children.push(...children); },
    setAttribute() {}, removeAttribute(name) { delete this[name]; } });
  const ids = new Map();
  const get = id => { if (!ids.has(id)) ids.set(id, element()); return ids.get(id); };
  let failAsset = false, constructed = 0, freed = 0, requests = 0;
  const wasmModule = { default: async () => {}, BrowserRuntime: { with_player_png(control) {
    constructed++;
    return { free() { freed++; }, handle(json) {
      requests++;
      const { request, request_id } = JSON.parse(json);
      let response;
      switch (request.type) {
        case 'capabilities': response = { operations: control ? ['step', 'invoke', 'inject_input', 'mutate', 'capture'] : ['capture'] }; break;
        case 'commands': response = { commands: [] }; break;
        case 'status': response = { current_frame: 0, paused: true }; break;
        case 'entities': response = { entities: [{ id: { index: 1, generation: 0 }, name: 'player', components: ['Position'] }] }; break;
        case 'entity': response = { components: { Position: { x: 0 } } }; break;
        case 'capture': response = { artifact: 'data:image/png;base64,test', width: 160, height: 112, checksum: 'abc' }; break;
        default: throw new Error(`unexpected ${request.type}`);
      }
      return JSON.stringify({ status: 'success', request_id, observed_frame: 0, response });
    } };
  } } };
  const context = vm.createContext({ document: { getElementById: get, createElement: element }, window: { addEventListener() {} }, wasmModule,
    loadPlayerPng: async () => { if (failAsset) throw new Error('Could not load ../assets/player.png: HTTP 404'); return new Uint8Array(); } });
  let script = readFileSync(new URL('./inspector.js', import.meta.url), 'utf8').replace(/^import.*\n/gm, '').replace('await import("./pkg/titan_browser.js")', 'wasmModule');
  script = script.replace('if (typeof document !== "undefined") initialize();', 'globalThis.ready = initialize();');
  vm.runInContext(script, context); await context.ready;
  const settle = () => new Promise(resolve => setImmediate(resolve));
  const click = id => get(id).handlers.click({ preventDefault() {} });
  click('enable-controls'); await settle();
  get('entities').children[0].handlers.click();
  assert.equal(get('mutation-form').hidden, false);
  assert.notEqual(get('entity-details').textContent, '');
  failAsset = true; click('enable-controls');
  // Clearing happens synchronously, while startup is still awaiting the fetch.
  assert.equal(get('mutation-form').hidden, true);
  assert.equal(get('entities').children.length, 0);
  assert.equal(get('entity-details').textContent, '');
  assert.equal(get('capabilities').textContent, '');
  assert.equal(get('reference-route').hidden, true);
  await settle();
  assert.equal(constructed, 2); assert.equal(freed, 2);
  assert.match(get('error').textContent, /player.png: HTTP 404/);
  const failure = get('error').textContent, before = requests;
  get('mutation-form').handlers.submit({ preventDefault() {} });
  click('refresh'); click('reference-route');
  assert.equal(requests, before);
  assert.equal(get('error').textContent, failure);
  assert.equal(get('enable-controls').textContent, 'Retry');
  failAsset = false; click('enable-controls'); await settle();
  assert.equal(constructed, 3);
  assert.equal(get('error').hidden, true);
  assert.equal(get('mode').textContent, 'Controls enabled');
  assert.equal(get('entities').children.length, 1);
  assert.equal(get('mutation-form').hidden, true);
});
