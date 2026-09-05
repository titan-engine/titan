import assert from 'node:assert/strict';
import test from 'node:test';
import { MAX_RECORDING_BYTES, readRecordingForSession } from './replay.mjs';

test('recording imports enforce byte bounds before and after reading', async () => {
  const session = { clock_epoch: () => '1', paused: () => true };
  let read = false;
  await assert.rejects(readRecordingForSession({ size: MAX_RECORDING_BYTES + 1, text() { read = true; } }, session, () => session), /2 MiB/);
  assert.equal(read, false);
  await assert.rejects(readRecordingForSession({ size: 1, text: async () => 'é'.repeat(MAX_RECORDING_BYTES) }, session, () => session), /2 MiB/);
  assert.equal(await readRecordingForSession({size:2, text:async () => '{}'}, session, () => session), '{}');
});

test('recording imports reject replacement or clock transitions during reading', async () => {
  for (const replace of [true, false]) {
    let epoch = '1';
    const session = { clock_epoch: () => epoch, paused: () => true };
    let current = session;
    let finish;
    const pending = readRecordingForSession({ size: 2, text: () => new Promise(resolve => { finish = resolve; }) }, session, () => current);
    if (replace) current = {}; else epoch = '2';
    finish('{}');
    await assert.rejects(pending, /session changed/);
  }
});
