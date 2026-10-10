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
  // Newlines are valid Git filenames on Unix, but not on Windows filesystems.
  const unusualName = process.platform === 'win32' ? 'space and unicode-é.rs' : 'space and\nnewline.rs';
  for (const name of ['src/lib.rs', 'Cargo.toml', unusualName]) {
    filenames.push(await source(root, name));
  }
  const untracked = await source(root, 'untracked.rs', 'untracked', false);
  // Even an accidentally tracked target file must not be a source input.
  const artifact = await source(root, 'target/debug/fingerprint', 'build bytes');
  assert.deepEqual(await saveSourceMtimes({ cwd: root, log: quiet }), { saved: 3 });
  const saved = await manifest(root);
  assert.equal(saved.version, 2);
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
  const link = { path: 'link', target: 'inside', resolved: 'inside' };
  const badManifests = [
    null, [], {}, { version: 3, files: [], links: [] }, { version: 2, files: {}, links: [] },
    { version: 2, files: [], links: [], extra: true },
    { version: 2, files: [good, good], links: [] },
    { version: 2, files: [], links: {} },
    { version: 2, files: [], links: [{ ...link, path: '../outside' }] },
    { version: 2, files: [], links: [{ ...link, target: '' }] },
    { version: 2, files: [], links: [{ ...link, target: 1 }] },
    { version: 2, files: [], links: [{ ...link, resolved: '../outside' }] },
    { version: 2, files: [], links: [{ path: 'link', target: 'inside' }] },
    { version: 2, files: [good], links: [{ ...link, path: good.path }] },
    { version: 2, files: [], links: [link, link] },
    // Pre-release manifests without link identity must not enable reuse.
    { version: 1, files: [good] },
    ...badEntries.map(bad => ({ version: 2, files: [good, bad], links: [] })),
  ];
  for (const bad of badManifests) {
    await putManifest(root, bad);
    await fs.utimes(filename, checkoutTime, checkoutTime);
    await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /Invalid source mtime manifest/);
    near((await fs.stat(filename)).mtimeMs, checkoutTime.getTime());
  }
  await fs.writeFile(path.join(root, MANIFEST_PATH), '{"version":2,"links":[],"files":[{"path":"src/lib.rs","sha256":"' + good.sha256 + '","mtimeMs":1e400}]}');
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
  await putManifest(root, { version: 2, links: [], files: [
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

test('unchanged internal links are allowed; changed links and regular-to-link transitions touch every source', async t => {
  const root = await repository(t);
  const regular = await source(root, 'regular.rs');
  await source(root, 'replacement.rs', 'different source');
  try {
    await fs.symlink('regular.rs', path.join(root, 'link.rs'));
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('Symlink creation requires privileges');
    throw error;
  }
  execFileSync('git', ['add', '--', 'link.rs'], { cwd: root });
  await saveSourceMtimes({ cwd: root, log: quiet });
  const saved = await manifest(root);
  assert.deepEqual(saved.links, [{ path: 'link.rs', target: 'regular.rs', resolved: 'regular.rs' }]);
  const linkTime = (await fs.lstat(path.join(root, 'link.rs'))).mtimeMs;
  assert.deepEqual(await restoreSourceMtimes({ cwd: root, log: quiet }), {
    cacheMiss: false, restored: 2, touched: 0,
  });
  assert.equal((await fs.lstat(path.join(root, 'link.rs'))).mtimeMs, linkTime);
  near((await fs.stat(regular)).mtimeMs, oldTime.getTime());
  await fs.unlink(path.join(root, 'link.rs'));
  await fs.symlink('replacement.rs', path.join(root, 'link.rs'));
  let start = Date.now();
  assert.deepEqual(await restoreSourceMtimes({ cwd: root, log: quiet }), {
    cacheMiss: true, restored: 0, touched: 2,
  });
  await assertTouched(regular, start, Date.now());
  await fs.unlink(path.join(root, 'link.rs'));
  await fs.symlink('regular.rs', path.join(root, 'link.rs'));
  await fs.unlink(regular);
  await fs.symlink('replacement.rs', regular);
  start = Date.now();
  assert.deepEqual(await restoreSourceMtimes({ cwd: root, log: quiet }), {
    cacheMiss: true, restored: 0, touched: 1,
  });
  await assertTouched(path.join(root, 'replacement.rs'), start, Date.now());
});

test('link resolution through an untracked intermediate cannot change silently', async t => {
  const root = await repository(t);
  await source(root, 'sources/a.rs', 'original library');
  await source(root, 'sources/b.rs', 'different library');
  try {
    await fs.symlink('sources/a.rs', path.join(root, 'alias.rs'));
    await fs.symlink('alias.rs', path.join(root, 'leaf.rs'));
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('Symlink creation requires privileges');
    throw error;
  }
  execFileSync('git', ['add', '--', 'leaf.rs'], { cwd: root });
  await saveSourceMtimes({ cwd: root, log: quiet });
  assert.deepEqual((await manifest(root)).links, [{ path: 'leaf.rs', target: 'alias.rs', resolved: 'sources/a.rs' }]);
  await fs.unlink(path.join(root, 'alias.rs'));
  await fs.symlink('sources/b.rs', path.join(root, 'alias.rs'));
  const start = Date.now();
  assert.deepEqual(await restoreSourceMtimes({ cwd: root, log: quiet }), {
    cacheMiss: true, restored: 0, touched: 2,
  });
  await assertTouched(path.join(root, 'sources/a.rs'), start, Date.now());
});

test('pre-release manifests without symlink identity fail closed, including existing internal links', async t => {
  const root = await repository(t);
  const regular = await source(root, 'regular.rs');
  try {
    await fs.symlink('regular.rs', path.join(root, 'link.rs'));
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('Symlink creation requires privileges');
    throw error;
  }
  execFileSync('git', ['add', '--', 'link.rs'], { cwd: root });
  await putManifest(root, { version: 1, files: [entry('regular.rs')] });
  await fs.utimes(regular, checkoutTime, checkoutTime);
  await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /Invalid source mtime manifest schema/);
  near((await fs.stat(regular)).mtimeMs, checkoutTime.getTime());
});

test('source links outside the checkout and symlink-parent transitions fail closed', async t => {
  const root = await repository(t);
  const external = await fs.mkdtemp(path.join(os.tmpdir(), 'cache-mtimes-outside-'));
  t.after(() => fs.rm(external, { recursive: true, force: true }));
  const outside = await source(external, 'lib.rs', 'outside', false);
  await source(root, 'src/lib.rs');
  await saveSourceMtimes({ cwd: root, log: quiet });
  try {
    await fs.symlink(outside, path.join(root, 'link.rs'));
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('Symlink creation requires privileges');
    throw error;
  }
  execFileSync('git', ['add', '--', 'link.rs'], { cwd: root });
  await assert.rejects(saveSourceMtimes({ cwd: root, log: quiet }), /Source symlink leaves the checkout/);
  await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /Source symlink leaves the checkout/);
  near((await fs.stat(outside)).mtimeMs, oldTime.getTime());
  execFileSync('git', ['rm', '--cached', '--', 'link.rs'], { cwd: root });
  await fs.mkdir(path.join(root, 'replacement'));
  await fs.writeFile(path.join(root, 'replacement/lib.rs'), 'different source');
  await fs.rm(path.join(root, 'src'), { recursive: true });
  await fs.symlink(path.join(root, 'replacement'), path.join(root, 'src'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(restoreSourceMtimes({ cwd: root, log: quiet }), /Tracked source has a symlink parent/);
  await assert.rejects(saveSourceMtimes({ cwd: root, log: quiet }), /Tracked source has a symlink parent/);
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
  const original = JSON.stringify({ version: 2, links: [], files: [entry('src/lib.rs')] });
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
