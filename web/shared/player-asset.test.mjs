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
