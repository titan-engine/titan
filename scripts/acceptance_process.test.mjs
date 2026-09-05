import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFile, run, timeoutSeconds } from './acceptance_process.mjs';

const node = process.execPath;
test('preserves stdout, stderr and expected nonzero results', async () => {
  const result = await run(node, ['-e', 'console.log("output"); console.error("diagnostic"); process.exitCode=7'], { encoding: 'utf8' });
  assert.equal(result.status, 7);
  assert.equal(result.stdout, 'output\n');
  assert.equal(result.stderr, 'diagnostic\n');
  await assert.rejects(execFile(node, ['-e', 'process.exit(2)']), /command failed \(2\)/);
});

test('separates build/runtime settings and rejects invalid deadlines', () => {
  const before = { ...process.env };
  try {
    process.env.TITAN_BUILD_TIMEOUT_SECONDS = '123';
    process.env.TITAN_RUNTIME_TIMEOUT_SECONDS = '4';
    assert.equal(timeoutSeconds('build'), 123);
    assert.equal(timeoutSeconds('runtime'), 4);
    for (const value of ['0', '-1', 'NaN', 'Infinity', '']) {
      process.env.TITAN_RUNTIME_TIMEOUT_SECONDS = value;
      assert.throws(() => timeoutSeconds('runtime'), /finite and positive/);
    }
  } finally {
    for (const name of ['TITAN_BUILD_TIMEOUT_SECONDS', 'TITAN_RUNTIME_TIMEOUT_SECONDS']) {
      if (before[name] === undefined) delete process.env[name];
      else process.env[name] = before[name];
    }
  }
});

for (const leaderExits of [false, true]) {
  test(`timeout kills pipe-holding descendant (leader exits: ${leaderExits})`, async () => {
    const source = `
      const { spawn } = require('node:child_process');
      const child = spawn(process.execPath, ['-e', 'process.on("SIGTERM",()=>{});setInterval(()=>{},1000)'], {stdio:['ignore','inherit','inherit']});
      console.log(child.pid);
      ${leaderExits ? 'child.unref();' : 'setInterval(()=>{},1000);'}
    `;
    let expired;
    await assert.rejects(run(node, ['-e', source], { timeout: 300, encoding: 'utf8' }), error => {
      expired = error;
      return /runtime phase timed out/.test(error.message);
    });
    const pid = Number(expired.stdout.trim());
    assert.ok(pid > 0, 'fixture reports descendant PID before hanging');
    // A killed orphan can briefly remain as a zombie awaiting its adopter.
    const state = await run('ps', ['-o', 'stat=', '-p', String(pid)], { encoding: 'utf8' });
    assert.ok(state.status !== 0 || state.stdout.trim().startsWith('Z'), `descendant survived: ${state.stdout}`);
  });
}

test('launch failures settle without waiting for deadline', async () => {
  await assert.rejects(run('/nonexistent/titan-acceptance-command'), { code: 'ENOENT' });
});

test('nested helpers expire before their owner deadline', async () => {
  const helper = new URL('./acceptance_process.mjs', import.meta.url).href;
  const source = `
    import { run } from ${JSON.stringify(helper)};
    try { await run(process.execPath, ['-e', 'setInterval(()=>{},1000)']); }
    catch (error) { console.log(error.message); process.exitCode = 23; }
  `;
  const result = await run(node, ['--input-type=module', '-e', source], { timeout: 1000, encoding: 'utf8' });
  assert.equal(result.status, 23);
  assert.match(result.stdout, /runtime phase timed out/);
});
