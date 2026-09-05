import assert from 'node:assert/strict';
import test from 'node:test';
import { MAX_SAVE_BYTES, readSaveForSession } from './save.mjs';

function fixture() {
  const state = { paused: true, enabled: true, epoch: '1' };
  const session = { paused: () => state.paused, control_enabled: () => state.enabled, clock_epoch: () => state.epoch };
  return { state, session };
}

test('save files are bounded before reading and again as UTF-8', async () => {
  const { session } = fixture();
  let read = false;
  await assert.rejects(readSaveForSession({ size: MAX_SAVE_BYTES + 1, text() { read = true; } }, session, () => session), /64 KiB/);
  assert.equal(read, false);
  await assert.rejects(readSaveForSession({ size: 1, text: async () => 'é'.repeat(40000) }, session, () => session), /64 KiB/);
  await assert.rejects(readSaveForSession({ size: 1, text: async () => '{' }, session, () => session), SyntaxError);
  assert.deepEqual(await readSaveForSession({ size: 2, text: async () => '{}' }, session, () => session), {});
});

test('load requires paused controls and rejects changes during asynchronous file reading', async () => {
  for (const change of ['resume', 'revoke', 'restart', 'replace']) {
    const { session, state } = fixture();
    let current = session;
    let finish;
    const pending = readSaveForSession({ size: 2, text: () => new Promise(resolve => { finish = resolve; }) }, session, () => current);
    if (change === 'resume') state.paused = false;
    if (change === 'revoke') state.enabled = false;
    if (change === 'restart') state.epoch = '2';
    if (change === 'replace') current = {};
    finish('{}');
    await assert.rejects(pending, /changed while reading/, change);
  }
  const { session, state } = fixture();
  state.enabled = false;
  await assert.rejects(readSaveForSession({ size: 2, text: async () => '{}' }, session, () => session), /Pause.*enable/);
});
