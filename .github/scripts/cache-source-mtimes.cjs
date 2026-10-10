// Run `node .github/scripts/cache-source-mtimes.cjs restore` after cache restore,
// and `... save` after successful builds, immediately before cache upload.
// Only source mtimes are restored: build artifacts/fingerprints stay untouched.
const { execFile } = require('node:child_process');
const { createHash, randomUUID } = require('node:crypto');
const { constants } = require('node:fs');
const fs = require('node:fs/promises');
const path = require('node:path');
const { promisify } = require('node:util');

const exec = promisify(execFile);
const MANIFEST_PATH = 'target/cache-source-mtimes.json';
const READ_FLAGS = constants.O_RDONLY | (constants.O_NOFOLLOW || 0);

async function repositoryRoot(cwd) {
  const { stdout } = await exec('git', ['rev-parse', '--show-toplevel'], { cwd });
  return fs.realpath(stdout.trimEnd());
}

function safeSourcePath(name) {
  // Git uses slash-separated, repository-relative names on every platform.
  // Reject backslashes too, so a manifest cannot change meaning on Windows.
  return typeof name === 'string' && name.length > 0 &&
    !name.includes('\\') && !name.includes('\0') && !name.includes(':') &&
    !path.posix.isAbsolute(name) &&
    name.split('/').every(part => part && part !== '.' && part !== '..') &&
    name !== 'target' && !name.startsWith('target/');
}

/** Return the Git allowlist, not a recursive filesystem walk. */
async function listTrackedFiles(root) {
  const { stdout } = await exec('git', ['ls-files', '-z'], {
    cwd: root, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024,
  });
  const names = stdout.toString('utf8');
  if (!Buffer.from(names, 'utf8').equals(stdout)) {
    throw new Error('Git paths must be valid UTF-8');
  }
  return [...new Set(names.split('\0').filter(Boolean))].filter(name => {
    // target must never be treated as source, even if accidentally tracked.
    if (name === 'target' || name.startsWith('target/')) return false;
    if (!safeSourcePath(name)) throw new Error(`Unsafe tracked path: ${JSON.stringify(name)}`);
    return true;
  });
}

async function lstatOrMissing(filename) {
  try {
    return await fs.lstat(filename);
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

// Check every component, not just the leaf: a directory replaced by a symlink
// must not grant access to files outside the checkout. Deleted files and gitlink
// directories are also skipped. The checkout must not be edited concurrently.
async function openSource(root, name, restore = false, onSymlink = async () => {}) {
  const parts = name.split('/');
  let filename = root;
  for (let i = 0; i < parts.length; i++) {
    filename = path.join(filename, parts[i]);
    const stat = await lstatOrMissing(filename);
    if (!stat) return null;
    if (stat.isSymbolicLink()) {
      await onSymlink(parts.slice(0, i + 1).join('/'), filename);
      return null;
    }
    if (i < parts.length - 1 ? !stat.isDirectory() : !stat.isFile()) return null;
  }
  // Windows futimes requires a writable handle (FILE_WRITE_ATTRIBUTES).
  // Keep save/hash-only opens read-only, and never write source contents.
  const flags = restore && process.platform === 'win32'
    ? constants.O_RDWR | (constants.O_NOFOLLOW || 0)
    : READ_FLAGS;
  const handle = await fs.open(filename, flags);
  if (!(await handle.stat()).isFile()) {
    await handle.close();
    return null;
  }
  return handle;
}

async function hashSource(handle) {
  const before = await handle.stat();
  const hash = createHash('sha256');
  // Streaming bounds memory use even for large tracked assets.
  for await (const chunk of handle.createReadStream({ autoClose: false })) {
    hash.update(chunk);
  }
  const after = await handle.stat();
  if (before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs) {
    throw new Error('Source changed while hashing; run without concurrent source edits');
  }
  return { sha256: hash.digest('hex'), stat: after };
}

function exactKeys(value, keys) {
  return value !== null && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
}

// Do not follow links to read or timestamp their destinations. Existing links
// inside the checkout are allowed, but changing one must invalidate the cache.
async function sourceLink(root, name, filename) {
  const target = await fs.readlink(filename);
  const resolved = await fs.realpath(filename);
  const relative = path.relative(root, resolved);
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`Source symlink leaves the checkout: ${name}`);
  }
  return { path: name, target };
}

/** Validate the entire cache manifest before changing any source mtime. */
function validateManifest(manifest) {
  if ((!exactKeys(manifest, ['version', 'files']) && !exactKeys(manifest, ['version', 'files', 'links'])) || manifest.version !== 1 || !Array.isArray(manifest.files)) {
    throw new Error('Invalid source mtime manifest schema');
  }
  const records = new Map();
  for (const entry of manifest.files) {
    if (!exactKeys(entry, ['path', 'sha256', 'mtimeMs']) || !safeSourcePath(entry.path) ||
        typeof entry.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(entry.sha256) ||
        typeof entry.mtimeMs !== 'number' || !Number.isFinite(entry.mtimeMs) ||
        entry.mtimeMs < 0 || entry.mtimeMs > 8.64e15 || records.has(entry.path)) {
      throw new Error('Invalid source mtime manifest entry');
    }
    records.set(entry.path, entry);
  }
  const links = new Map();
  if (manifest.links !== undefined && !Array.isArray(manifest.links)) {
    throw new Error('Invalid source mtime manifest links');
  }
  for (const entry of manifest.links || []) {
    if (!exactKeys(entry, ['path', 'target']) || !safeSourcePath(entry.path) ||
        typeof entry.target !== 'string' || !entry.target || entry.target.includes('\0') ||
        links.has(entry.path) || records.has(entry.path)) {
      throw new Error('Invalid source mtime manifest link');
    }
    links.set(entry.path, entry);
  }
  return { files: records, links };
}

async function targetDirectory(root, create) {
  const target = path.join(root, 'target');
  let stat = await lstatOrMissing(target);
  if (!stat && create) {
    await fs.mkdir(target);
    stat = await fs.lstat(target);
  }
  if (stat && !stat.isDirectory()) throw new Error('target must be a real directory, not a symlink');
  return stat ? target : null;
}

async function readManifest(root) {
  const target = await targetDirectory(root, false);
  if (!target) return null;
  const filename = path.join(root, MANIFEST_PATH);
  const stat = await lstatOrMissing(filename);
  if (!stat) return null;
  if (!stat.isFile()) throw new Error('Source mtime manifest must be a regular file, not a symlink');
  const handle = await fs.open(filename, READ_FLAGS);
  try {
    return validateManifest(JSON.parse(await handle.readFile('utf8')));
  } finally {
    await handle.close();
  }
}

/** Snapshot tracked regular source files after builds, into the target cache. */
async function saveSourceMtimes({ cwd = process.cwd(), log = console.log } = {}) {
  const root = await repositoryRoot(cwd);
  const files = [];
  const links = [];
  for (const name of await listTrackedFiles(root)) {
    const handle = await openSource(root, name, false, async (linkName, filename) => {
      if (linkName !== name) throw new Error(`Tracked source has a symlink parent: ${name}`);
      links.push(await sourceLink(root, linkName, filename));
    });
    if (!handle) continue;
    try {
      const { sha256, stat } = await hashSource(handle);
      files.push({ path: name, sha256, mtimeMs: stat.mtimeMs });
    } finally {
      await handle.close();
    }
  }
  const manifest = { version: 1, files, links };
  validateManifest(manifest);
  const target = await targetDirectory(root, true);
  const destination = path.join(root, MANIFEST_PATH);
  const previous = await lstatOrMissing(destination);
  if (previous && !previous.isFile()) throw new Error('Source mtime manifest must be a regular file, not a symlink');
  const temporary = path.join(target, `.cache-source-mtimes-${randomUUID()}.tmp`);
  try {
    await fs.writeFile(temporary, `${JSON.stringify(manifest)}\n`, { flag: 'wx' });
    await fs.rename(temporary, destination);
  } finally {
    await fs.rm(temporary, { force: true });
  }
  log(`Saved source mtimes for ${files.length} tracked file(s) to ${MANIFEST_PATH}`);
  return { saved: files.length };
}

/** Restore identical sources; force changed/new sources to invalidate Cargo. */
async function restoreSourceMtimes({ cwd = process.cwd(), log = console.log } = {}) {
  const root = await repositoryRoot(cwd);
  // Validate everything first: malformed cache data is fatal, never a partial
  // restore or a silent fallback to potentially unsafe checkout timestamps.
  const records = await readManifest(root);
  if (!records) log(`Source mtime cache miss: ${MANIFEST_PATH} is absent; touching tracked sources`);
  const now = Date.now();
  let restored = 0;
  let touched = 0;
  // Iterate ONLY the current Git allowlist. Safe names left over from another
  // commit's manifest (deleted/renamed sources) are ignored, never opened.
  for (const name of await listTrackedFiles(root)) {
    const handle = await openSource(root, name, true, async (linkName, filename) => {
      const link = await sourceLink(root, linkName, filename);
      // Missing manifests touch every regular source/Cargo.toml, forcing a
      // rebuild. With a manifest, never silently reuse artifacts across a
      // regular-file -> symlink transition or a changed/new symlink target.
      if (records && (linkName !== name || records.links.get(name)?.target !== link.target)) {
        throw new Error(`Source symlink changed; rebuild the cache before restoring mtimes: ${name}`);
      }
    });
    if (!handle) continue;
    try {
      const entry = records?.files.get(name);
      const { sha256, stat } = entry ? await hashSource(handle) : { stat: await handle.stat() };
      const matches = entry && entry.sha256 === sha256;
      await handle.utimes(stat.atime, (matches ? entry.mtimeMs : now) / 1000);
      if (matches) restored++;
      else touched++;
    } finally {
      await handle.close();
    }
  }
  log(`Source mtimes: restored ${restored}, touched ${touched} changed/unrecorded tracked file(s)`);
  return { cacheMiss: records === null, restored, touched };
}

async function main() {
  if (process.argv.length !== 3 || !['save', 'restore'].includes(process.argv[2])) {
    throw new Error('Usage: node .github/scripts/cache-source-mtimes.cjs <save|restore>');
  }
  await (process.argv[2] === 'save' ? saveSourceMtimes() : restoreSourceMtimes());
}

if (require.main === module) {
  main().catch(error => {
    console.error(`Source mtime cache error: ${error.message}`);
    process.exitCode = 1;
  });
}

module.exports = { MANIFEST_PATH, listTrackedFiles, validateManifest, saveSourceMtimes, restoreSourceMtimes };
