// Real WASM + native replay: changing a PNG requires no rebuild.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { execFile, run } from './acceptance_process.mjs';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';
const repo = fileURLToPath(new URL('../', import.meta.url));
const metadata = JSON.parse(await execFile('cargo', ['metadata', '--no-deps', '--format-version', '1'], { phase: 'build', cwd: repo, encoding: 'utf8' }));
const require = createRequire(import.meta.url);
const { BrowserRuntime, BrowserLiveRuntime, verify_recording_json_with_pngs } = require(resolve(metadata.target_directory, 'titan/browser-node/titan_browser.js'));
const original = readFileSync(join(repo, 'assets/player.png'));
const originalTree = readFileSync(join(repo, 'assets/tree.png'));
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) { crc ^= byte; for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0); }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type), data]);
  const head = Buffer.alloc(4), tail = Buffer.alloc(4);
  head.writeUInt32BE(data.length); tail.writeUInt32BE(crc32(body));
  return Buffer.concat([head, body, tail]);
}
function png(width, height) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 6;
  const rows = Buffer.alloc(height * (1 + width * 4));
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) rows.set([240, 30, 180, 255], y * (1 + width * 4) + 1 + x * 4);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', header), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0))]);
}
const replacement = png(8, 8);
let sequence = 0;
function call(runtime, request, success = true) {
  const result = JSON.parse(runtime.handle(JSON.stringify({ schema_version: 1, request_id: `asset-${++sequence}`, request })));
  assert.equal(result.status, success ? 'success' : 'failure', JSON.stringify(result));
  return result;
}
const query = (runtime, name) => call(runtime, { type: 'query', name, arguments: {} }).response.value;
const capture = runtime => call(runtime, { type: 'capture' }).response.checksum;
const invoke = (runtime, name, args = {}) => call(runtime, { type: 'invoke', name, arguments: args });
function reference(runtime) {
  let frame = call(runtime, { type: 'status' }).observed_frame;
  for (const [action, ticks] of [['right', 2], ['down', 3], ['right', 6]]) for (let tick = 0; tick < ticks; tick++) {
    call(runtime, { type: 'inject_input', frame: ++frame, actions: { [action]: { kind: 'button', value: true } } });
  }
  call(runtime, { type: 'step', frames: 11 });
}
for (const factory of [(player, tree) => BrowserRuntime.with_pngs(true, player, tree), (player, tree) => BrowserLiveRuntime.with_pngs(player, tree)]) {
  for (const bytes of [new Uint8Array(), original.subarray(0, 20), Buffer.from('not a PNG'), new Uint8Array(256 * 1024 + 1), png(65, 1)]) {
    assert.throws(() => factory(bytes, originalTree), /player.png/);
    assert.throws(() => factory(original, bytes), /tree.png/);
  }
  factory(original, originalTree).free(); // Repaired construction after either source failed.
}
const procedural = new BrowserRuntime(true);
const loaded = BrowserRuntime.with_pngs(true, original, originalTree);
assert.equal(capture(loaded), capture(procedural));
reference(loaded); reference(procedural);
assert.equal(capture(loaded), 'f7a298f62ad75c1c');
assert.deepEqual(query(loaded, 'save'), query(procedural, 'save'));

await execFile('cargo', ['build', '--example', 'replay_rpg'], { phase: 'build', cwd: repo, stdio: 'inherit' });
const temporary = mkdtempSync(join(tmpdir(), 'titan-rpg-png-'));
try {
  const checksums = new Set([capture(procedural)]);
  for (const [playerBytes, treeBytes] of [[replacement, originalTree], [original, replacement], [replacement, replacement]]) {
    const changed = BrowserRuntime.with_pngs(true, playerBytes, treeBytes);
    reference(changed);
    assert.deepEqual(query(changed, 'save'), query(procedural, 'save'));
    const changedReference = capture(changed);
    assert.ok(!checksums.has(changedReference), 'each independently replaced sprite changes pixels');
    checksums.add(changedReference); changed.free();

    const live = BrowserLiveRuntime.with_pngs(playerBytes, treeBytes);
    live.set_control_enabled(true);
    const initialChecksum = capture(live);
    reference(live); assert.equal(capture(live), changedReference);
    const save = query(live, 'save');
    const recording = JSON.stringify(query(live, 'recording'));
    assert.equal(JSON.parse(verify_recording_json_with_pngs(recording, playerBytes, treeBytes)).verified, true);
    if (playerBytes === replacement) assert.throws(() => verify_recording_json_with_pngs(recording, original, treeBytes), /mismatch|asset/i);
    if (treeBytes === replacement) assert.throws(() => verify_recording_json_with_pngs(recording, playerBytes, originalTree), /mismatch|asset/i);
    // All reconstruction paths retain the complete startup pair.
    invoke(live, 'restart'); assert.equal(capture(live), initialChecksum);
    invoke(live, 'load_save', { save }); assert.equal(capture(live), changedReference);
    invoke(live, 'restart');
    live.load_recording(recording);
    while (!JSON.parse(live.playback_status()).complete) live.step_playback();
    assert.equal(JSON.parse(live.playback_status()).verified, true);
    assert.equal(capture(live), changedReference);
    live.restart_playback(); assert.equal(capture(live), initialChecksum);
    live.exit_playback(); assert.equal(capture(live), initialChecksum);
    reference(live); assert.equal(capture(live), changedReference); live.free();

    writeFileSync(join(temporary, 'player.png'), playerBytes);
    writeFileSync(join(temporary, 'tree.png'), treeBytes);
    const path = join(temporary, 'recording.json'); writeFileSync(path, recording);
    const executable = resolve(metadata.target_directory, 'debug/examples/replay_rpg');
    const native = JSON.parse(await execFile(executable, [path, '--assets-dir', temporary], { cwd: temporary, encoding: 'utf8' }));
    assert.equal(native.verified, true); assert.equal(native.checksum, changedReference);
    const wrong = await run(executable, [path, '--assets-dir', join(repo, 'assets')], { cwd: temporary, encoding: 'utf8' });
    assert.notEqual(wrong.status, 0); assert.match(wrong.stderr, /mismatch|asset/i);
    for (const [name, bytes] of [['player.png', playerBytes], ['tree.png', treeBytes]]) {
      rmSync(join(temporary, name));
      const missing = await run(executable, [path, '--assets-dir', temporary], { cwd: temporary, encoding: 'utf8' });
      assert.notEqual(missing.status, 0); assert.ok(missing.stderr.includes(name));
      writeFileSync(join(temporary, name), bytes);
    }
  }
} finally { rmSync(temporary, { recursive: true, force: true }); }
loaded.free(); procedural.free();
console.log('Two-sprite WASM/native acceptance passed: reference pixels, independent replacements, bounded failures, retained pair and replay verification.');
