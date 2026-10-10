const assert = require('node:assert/strict');
const { execFileSync, spawnSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { MANIFEST_PATH, saveSourceMtimes, restoreSourceMtimes } = require('./cache-source-mtimes.cjs');

const script = path.join(__dirname, 'cache-source-mtimes.cjs');
const oldTime = new Date('2020-01-02T03:04:05.123Z');
const checkoutTime = new Date('2021-01-02T03:04:05Z');
const quiet = () => {};

async function repository(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cache-source-mtimes-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '--quiet', root]);
  return root;
}

async function source(root, name, contents = 'source bytes', tracked = true) {
  const filename = path.join(root, name);
  await fs.mkdir(path.dirname(filename), { recursive: true });
  await fs.writeFile(filename, contents);
  await fs.utimes(filename, oldTime, oldTime);
  if (tracked) execFileSync('git', ['add', '--', name], { cwd: root });
  return filename;
}

async function manifest(root) {
  return JSON.parse(await fs.readFile(path.join(root, MANIFEST_PATH), 'utf8'));
}

async function putManifest(root, data) {
  await fs.mkdir(path.join(root, 'target'), { recursive: true });
  await fs.writeFile(path.join(root, MANIFEST_PATH), JSON.stringify(data));
}

function entry(name, contents = 'source bytes') {
  return { path: name, sha256: createHash('sha256').update(contents).digest('hex'), mtimeMs: oldTime.getTime() };
}

function near(actual, expected) {
  // Node utimes and filesystems differ in sub-millisecond precision.
  assert.ok(Math.abs(actual - expected) < 1, `${actual} should be near ${expected}`);
}

async function assertTouched(filename, start, end) {
  const { mtimeMs } = await fs.stat(filename);
  assert.ok(mtimeMs >= start - 1 && mtimeMs <= end + 1, `${mtimeMs} should be NOW (${start}..${end})`);
}

test('save/restore resets identical freshly checked-out sources without touching artifacts', async t => {
  const root = await repository(t);
  const filenames = [];
  for (const name of ['src/lib.rs', 'Cargo.toml', 'space and\nnewline.rs']) {
    filenames.push(await source(root, name));
  }
  const untracked = await source(root, 'untracked.rs', 'untracked', false);
  // Even an accidentally tracked target file must not be a source input.
  const artifact = await source(root, 'target/debug/fingerprint', 'build bytes');
  assert.deepEqual(await saveSourceMtimes({ cwd: root, log: quiet }), { saved: 3 });
  const saved = await manifest(root);
  assert.equal(saved.version, 1);
  assert.equal(saved.files.length, 3);
  for (const record of saved.files) {
    assert.equal(record.sha256, entry(record.path).sha256);
    near(record.mtimeMs, oldTime.getTime());
  }
  for (const filename of filenames) await fs.utimes(filename, checkoutTime, checkoutTime);
  assert.deepEqual(await restoreSourceMtimes({ cwd: path.join(root, 'src'), log: quiet }), {
    cacheMiss: false, restored: 3, touched: 0,
  });
  for (const filename of filenames) near((await fs.stat(filename)).mtimeMs, oldTime.getTime());
  near((await fs.stat(untracked)).mtimeMs, oldTime.getTime());
  near((await fs.stat(artifact)).mtimeMs, oldTime.getTime());
  assert.equal(await fs.readFile(artifact, 'utf8'), 'build bytes');
});

test('changed bytes and newly tracked sources become dirty even when checkout predates cache build', async t => {
  const root = await repository(t);
  const changed = await source(root, 'src/leaf.rs', 'original');
  const unchanged = await source(root, 'src/lib.rs');
  await saveSourceMtimes({ cwd: root, log: quiet });
  await fs.writeFile(changed, 'modified'); // Same length: hash, not size, decides.
  const newlyTracked = await source(root, 'src/new.rs');
  for (const filename of [changed, newlyTracked]) {
    await fs.utimes(filename, new Date('2010-01-01'), new Date('2010-01-01'));
  }
  const start = Date.now();
  const result = await restoreSourceMtimes({ cwd: root, log: quiet });
  const end = Date.now();
  assert.deepEqual(result, { cacheMiss: false, restored: 1, touched: 2 });
  await assertTouched(changed, start, end);
  await assertTouched(newlyTracked, start, end);
  near((await fs.stat(unchanged)).mtimeMs, oldTime.getTime());
  assert.equal(await fs.readFile(changed, 'utf8'), 'modified');
});

test('missing manifest is a graceful CLI cache miss that invalidates all tracked sources', async t => {
  const root = await repository(t);
  const filename = await source(root, 'src/lib.rs');
  const untracked = await source(root, 'other.rs', 'other', false);
  const start = Date.now();
  const result = spawnSync(process.execPath, [script, 'restore'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /cache miss/);
  await assertTouched(filename, start, Date.now());
  near((await fs.stat(untracked)).mtimeMs, oldTime.getTime());
  // Also graceful when target exists but the manifest does not (rollout).
  await fs.mkdir(path.join(root, 'target'));
  const messages = [];
  assert.deepEqual(await restoreSourceMtimes({ cwd: root, log: message => messages.push(message) }), {
    cacheMiss: true, restored: 0, touched: 1,
  });
  assert.match(messages[0], /cache miss/);
});

test('malformed manifests fail before changing any source timestamps', async t => {
  const root = await repository(t);
  const filename = await source(root, 'src/lib.rs');
  const good = entry('src/lib.rs');
  const badEntries = [
    { ...good, path: '../outside.rs' },
    { ...good, path: '/absolute.rs' },
    { ...good, path: 'src/../../outside.rs' },
    { ...good, path: 'src//lib.rs' },
    { ...good, path: './src/lib.rs' },
    { ...good, path: 'src\\lib.rs' },
    { ...good, path: 'C:/outside.rs' },
    { ...good, path: 'src/lib.rs\0' },
    { ...good, path: 'target/cache-source-mtimes.json' },
    { ...good, sha256: 'not a digest' },
    { ...good, sha256: 'a'.repeat(63) },
    { ...good, sha256: 'A'.repeat(64) },
    { ...good, sha256: 1 },
    { ...good, mtimeMs: '1234' },
    { ...good, mtimeMs: null },
    { ...good, mtimeMs: -1 },
    { ...good, mtimeMs: 8.64e15 + 1 },
    { ...good, extra: true },
    { path: good.path, sha256: good.sha256 },
    null,
  ];
  const badManifests = [
    null, [], {}, { version: 2, files: [] }, { version: 1, files: {} },
    { version: 1, files: [], extra: true },
    { version: 1, files: [good, good] },
    ...badEntries.map(bad => ({ version: 1, files: [good, bad] })),
  ];
  for (const bad of badManifests) {
    await putManifest(root, bad);
    await fs.utimes(filename, checkoutTime, checkoutTime);
    await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /Invalid source mtime manifest/);
    near((await fs.stat(filename)).mtimeMs, checkoutTime.getTime());
  }
  await fs.writeFile(path.join(root, MANIFEST_PATH), '{"version":1,"files":[{"path":"src/lib.rs","sha256":"' + good.sha256 + '","mtimeMs":1e400}]}');
  await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /Invalid source mtime manifest/);
  near((await fs.stat(filename)).mtimeMs, checkoutTime.getTime());
  await fs.writeFile(path.join(root, MANIFEST_PATH), '{broken JSON');
  const result = spawnSync(process.execPath, [script, 'restore'], { cwd: root, encoding: 'utf8' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Source mtime cache error/);
  near((await fs.stat(filename)).mtimeMs, checkoutTime.getTime());
});

test('manifest-only names are never opened; removed files allow cross-commit cache reuse', async t => {
  const root = await repository(t);
  const tracked = await source(root, 'src/lib.rs');
  const untracked = await source(root, 'untracked.rs', 'source bytes', false);
  await putManifest(root, { version: 1, files: [
    entry('src/lib.rs'), entry('untracked.rs'), entry('deleted.rs'), entry('.git/config'),
  ] });
  await fs.utimes(tracked, checkoutTime, checkoutTime);
  await fs.utimes(untracked, checkoutTime, checkoutTime);
  const gitConfig = await fs.stat(path.join(root, '.git/config'));
  const result = await restoreSourceMtimes({ cwd: root, log: quiet });
  assert.deepEqual(result, { cacheMiss: false, restored: 1, touched: 0 });
  near((await fs.stat(tracked)).mtimeMs, oldTime.getTime());
  near((await fs.stat(untracked)).mtimeMs, checkoutTime.getTime());
  assert.equal((await fs.stat(path.join(root, '.git/config'))).mtimeMs, gitConfig.mtimeMs);
});

test('tracked leaf and parent symlinks are skipped without touching their destinations', async t => {
  const root = await repository(t);
  const external = await fs.mkdtemp(path.join(os.tmpdir(), 'cache-mtimes-outside-'));
  t.after(() => fs.rm(external, { recursive: true, force: true }));
  const outside = await source(external, 'lib.rs', 'source bytes', false);
  const nested = await source(root, 'src/lib.rs');
  const regular = await source(root, 'regular.rs');
  try {
    await fs.symlink(outside, path.join(root, 'link.rs'));
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('Symlink creation requires privileges');
    throw error;
  }
  execFileSync('git', ['add', '--', 'link.rs'], { cwd: root });
  await saveSourceMtimes({ cwd: root, log: quiet });
  assert.ok(!(await manifest(root)).files.some(record => record.path === 'link.rs'));
  await fs.rm(path.dirname(nested), { recursive: true });
  await fs.symlink(external, path.join(root, 'src'), process.platform === 'win32' ? 'junction' : 'dir');
  // Even an injected, valid matching record for a tracked symlink cannot be used.
  const saved = await manifest(root);
  saved.files.push(entry('link.rs'));
  await putManifest(root, saved);
  assert.deepEqual(await restoreSourceMtimes({ cwd: root, log: quiet }), {
    cacheMiss: false, restored: 1, touched: 0,
  });
  near((await fs.stat(outside)).mtimeMs, oldTime.getTime());
  near((await fs.stat(regular)).mtimeMs, oldTime.getTime());
  assert.deepEqual(await saveSourceMtimes({ cwd: root, log: quiet }), { saved: 1 });
});

test('target and manifest symlinks fail closed instead of reading or writing outside the checkout', async t => {
  const root = await repository(t);
  await source(root, 'src/lib.rs');
  const outside = await fs.mkdtemp(path.join(os.tmpdir(), 'cache-mtimes-target-'));
  t.after(() => fs.rm(outside, { recursive: true, force: true }));
  try {
    await fs.symlink(outside, path.join(root, 'target'), process.platform === 'win32' ? 'junction' : 'dir');
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('Symlink creation requires privileges');
    throw error;
  }
  await assert.rejects(saveSourceMtimes({ cwd: root, log: quiet }), /target must be a real directory/);
  await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /target must be a real directory/);
  assert.deepEqual(await fs.readdir(outside), []);
  await fs.unlink(path.join(root, 'target'));
  await fs.mkdir(path.join(root, 'target'));
  const externalManifest = path.join(outside, 'manifest.json');
  const original = JSON.stringify({ version: 1, files: [entry('src/lib.rs')] });
  await fs.writeFile(externalManifest, original);
  await fs.symlink(externalManifest, path.join(root, MANIFEST_PATH));
  await assert.rejects(saveSourceMtimes({ cwd: root, log: quiet }), /manifest must be a regular file/);
  await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /manifest must be a regular file/);
  assert.equal(await fs.readFile(externalManifest, 'utf8'), original);
});

test('large source files are hashed correctly and the save CLI writes the fixed cache path', async t => {
  const root = await repository(t);
  const contents = Buffer.alloc(5 * 1024 * 1024, 42);
  await source(root, 'large.bin', contents);
  const result = spawnSync(process.execPath, [script, 'save'], { cwd: root, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal((await manifest(root)).files[0].sha256, createHash('sha256').update(contents).digest('hex'));
  const invalid = spawnSync(process.execPath, [script, 'restore', '../other.json'], { cwd: root, encoding: 'utf8' });
  assert.notEqual(invalid.status, 0);
  assert.match(invalid.stderr, /Usage:/);
});
