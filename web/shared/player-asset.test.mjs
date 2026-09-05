import assert from 'node:assert/strict';
import test from 'node:test';
import { loadPlayerPng, PLAYER_PNG_MAX_BYTES } from './player-asset.mjs';

test('player asset fetch uses URL and bypasses stale HTTP cache', async () => {
  let options;
  const bytes = await loadPlayerPng({ fetch: async (url, config) => {
    assert.equal(url, '../assets/player.png'); options = config;
    return new Response(new Uint8Array([1, 2, 3]));
  } });
  assert.deepEqual(bytes, new Uint8Array([1, 2, 3]));
  assert.equal(options.cache, 'no-store');
  assert.equal(PLAYER_PNG_MAX_BYTES, 256 * 1024);
});

test('player asset reports HTTP and transport failures without a fallback', async () => {
  await assert.rejects(loadPlayerPng({ fetch: async () => new Response('', { status: 404 }) }), /assets\/player.png: HTTP 404/);
  await assert.rejects(loadPlayerPng({ fetch: async () => { throw new Error('network offline'); } }), /network offline/);
});

test('player asset rejects oversized headers and streamed bytes', async () => {
  await assert.rejects(loadPlayerPng({ maxBytes: 2, fetch: async () => new Response('', { headers: { 'content-length': '3' } }) }), /exceeds 2 bytes/);
  let canceled = false;
  const stream = new ReadableStream({ start(controller) { controller.enqueue(new Uint8Array(3)); }, cancel() { canceled = true; } });
  await assert.rejects(loadPlayerPng({ maxBytes: 2, fetch: async () => new Response(stream) }), /exceeds 2 bytes/);
  assert.equal(canceled, true);
});

test('player asset timeout covers connection and body download', async () => {
  const stall = (_url, { signal }) => new Promise((_resolve, reject) => signal.addEventListener('abort', () => reject(new Error('aborted'))));
  await assert.rejects(loadPlayerPng({ timeoutMs: 5, fetch: stall }), /timed out after 5 ms/);
  const bodyStall = async (_url, { signal }) => new Response(new ReadableStream({ start(controller) { signal.addEventListener('abort', () => controller.error(new Error('aborted'))); } }));
  await assert.rejects(loadPlayerPng({ timeoutMs: 5, fetch: bodyStall }), /timed out after 5 ms/);
});

test('pair fetch waits for both sources and applies a separate bound to each', async () => {
  const { loadRpgPngs } = await import('./player-asset.mjs');
  const urls = [];
  const pair = await loadRpgPngs({ maxBytes: 2, fetch: async url => {
    urls.push(url);
    return new Response(new Uint8Array(url.endsWith('player.png') ? [1, 2] : [3, 4]));
  } });
  assert.deepEqual(urls.sort(), ['../assets/player.png', '../assets/tree.png']);
  assert.deepEqual(pair, { player: new Uint8Array([1, 2]), tree: new Uint8Array([3, 4]) });
  for (const failed of ['player.png', 'tree.png']) {
    await assert.rejects(loadRpgPngs({ maxBytes: 2, fetch: async url => new Response(new Uint8Array(url.endsWith(failed) ? 3 : 2)) }), new RegExp(`${failed}: PNG exceeds 2 bytes`));
    await assert.rejects(loadRpgPngs({ fetch: async url => new Response('', { status: url.endsWith(failed) ? 404 : 200 }) }), new RegExp(`${failed}: HTTP 404`));
  }
});

test('a ready player never exposes a partial pair while tree is pending or timed out', async () => {
  const { loadRpgPngs } = await import('./player-asset.mjs');
  let releaseTree, ready = false;
  const pending = loadRpgPngs({ fetch: async url => url.endsWith('player.png')
    ? new Response(new Uint8Array([1]))
    : new Promise(resolve => { releaseTree = () => resolve(new Response(new Uint8Array([2]))); }) });
  pending.then(() => { ready = true; });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(ready, false);
  releaseTree();
  assert.deepEqual((await pending).tree, new Uint8Array([2]));
  await assert.rejects(loadRpgPngs({ timeoutMs: 5, fetch: async (url, { signal }) => {
    if (url.endsWith('player.png')) return new Response(new Uint8Array([1]));
    return new Promise((_resolve, reject) => signal.addEventListener('abort', () => reject(new Error('aborted'))));
  } }), /tree.png: timed out after 5 ms/);
});
