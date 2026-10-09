const assert = require('node:assert/strict');
const { test } = require('node:test');
const { cachesToDelete, prune } = require('./prune-actions-caches.cjs');

const prefix = 'Linux-stable--';
const key = `${prefix}new-manifest-lock-20261009`;
function cache(id, cacheKey = key, ref = 'refs/heads/main', createdAt = '2026-10-09T12:00:00Z') {
  return { id, key: cacheKey, ref, created_at: createdAt, version: 'test-version' };
}

function client(caches) {
  const deleted = [];
  const warnings = [];
  let listed;
  const github = {
    rest: { actions: {
      getActionsCacheList: Symbol('getActionsCacheList'),
      deleteActionsCacheById: async params => deleted.push(params),
    } },
    paginate: async (route, params) => {
      assert.equal(route, github.rest.actions.getActionsCacheList);
      listed = params;
      return caches;
    },
  };
  return {
    deleted, warnings,
    get listed() { return listed; },
    args: {
      github,
      context: { repo: { owner: 'titan-engine', repo: 'titan' }, ref: 'refs/heads/main' },
      core: { info() {}, warning: message => warnings.push(message) },
      prefix, currentKey: key, sleep: async () => {},
    },
  };
}

test('two same-day merges leave one cache per prefix without changing the newest restore match', () => {
  const prefixes = [
    prefix, 'Windows-stable--', 'macOS-stable--', 'Linux-nightly--', 'macOS-nightly--',
    'Linux-1.97.1--', 'Linux-nightly-wasm32-unknown-unknown-',
    'Linux-stable-wasm32-unknown-unknown-', 'Linux-stable-x86_64-unknown-none-',
    'Linux-stable-thumbv6m-none-eabi-', 'Linux-stable-aarch64-linux-android-',
    'macOS-stable-aarch64-apple-ios-sim-', 'Linux-apt-kisak_turtle-',
  ];
  let caches = [];
  let id = 0;
  for (const generation of [1, 2]) {
    for (const restorePrefix of prefixes) {
      const currentKey = `${restorePrefix}manifest-${generation}-lock-20261009`;
      caches.push(cache(++id, currentKey, 'refs/heads/main', `2026-10-09T1${generation}:00:00Z`));
      const newestRestore = caches.filter(c => c.key.startsWith(restorePrefix)).at(-1);
      const deleted = cachesToDelete(caches, restorePrefix, currentKey).map(c => c.id);
      caches = caches.filter(c => !deleted.includes(c.id));
      assert.deepEqual(caches.filter(c => c.key.startsWith(restorePrefix)), [newestRestore]);
    }
    assert.equal(caches.length, prefixes.length);
  }
});

test('only deletes older entries of the same prefix on main', () => {
  const old = cache(1, `${prefix}old`, undefined, '2026-10-08T12:00:00Z');
  const unrelated = [
    cache(3, key, 'refs/pull/43/merge'),
    cache(4, key, 'refs/heads/feature'),
    cache(5, 'Linux-stable-wasm32-unknown-unknown-old'),
    cache(6, 'Linux-nightly--old'),
    cache(7, 'Windows-stable--old'),
    cache(8, 'Linux-1.97.1--old'),
  ];
  const caches = [old, cache(2), ...unrelated];
  assert.deepEqual(cachesToDelete(caches, prefix, key), [old]);
  assert.equal(caches[0], old); // Does not sort/mutate the API snapshot.
});

test('an older manifest still gets a broad-prefix restore hit after its exact tier is pruned', () => {
  const old = cache(1, `${prefix}old-manifest-lock-20261009`, undefined, '2026-10-09T11:00:00Z');
  const newest = cache(2);
  const deleted = cachesToDelete([old, newest], prefix, key).map(c => c.id);
  const remaining = [old, newest].filter(c => !deleted.includes(c.id));
  for (const manifest of ['old-manifest', 'new-manifest']) {
    const restoreKeys = [`${prefix}${manifest}-`, prefix];
    const restored = restoreKeys.map(p => remaining.find(c => c.key.startsWith(p))).find(Boolean);
    assert.equal(restored, newest);
  }
});

test('an absent, failed, or not-yet-visible save preserves every fallback', () => {
  const old = cache(1, `${prefix}old`);
  assert.deepEqual(cachesToDelete([old], prefix, key), []);
  assert.deepEqual(cachesToDelete([old, cache(2, key, 'refs/heads/feature')], prefix, key), []);
  assert.deepEqual(cachesToDelete([], prefix, key), []);
});

test('keeps newer caches if an older producer finishes late, and ignores access time', () => {
  const oldRun = cache(1);
  oldRun.last_accessed_at = '2026-10-10T12:00:00Z';
  const newer = cache(2, `${prefix}newer`, undefined, '2026-10-09T13:00:00Z');
  assert.deepEqual(cachesToDelete([newer, oldRun], prefix, key), [oldRun]);
});

test('an exact-hit rerun cleans duplicates of the same version, including identical keys', () => {
  assert.deepEqual(cachesToDelete([cache(1), cache(2)], prefix, key), [cache(1)]);
  assert.deepEqual(cachesToDelete([cache(2)], prefix, key), []);
});

test('an incompatible current key cannot justify deleting a compatible fallback after a failed save', async () => {
  const old = cache(1, `${prefix}old`);
  const incompatible = { ...cache(2), version: 'different-paths-or-compression' };
  assert.deepEqual(cachesToDelete([old, incompatible], prefix, key), []);
  assert.deepEqual(cachesToDelete([cache(1), incompatible], prefix, key), []);
  assert.deepEqual(cachesToDelete([{ ...old, version: undefined }, cache(2)], prefix, key), []);
  const mock = client([old, incompatible]);
  await prune(mock.args);
  assert.deepEqual(mock.deleted, []);
  assert.match(mock.warnings[0], /Mixed or unknown cache versions/);
});

test('rejects empty prefixes, incomplete keys, and keys from another namespace', () => {
  for (const [p, k] of [['', key], [prefix, ''], [prefix, prefix], [prefix, 'Windows-stable--new']]) {
    assert.throws(() => cachesToDelete([], p, k), /complete cache key/);
  }
});

test('API listing is paginated and scoped; deletion uses IDs from the complete snapshot', async () => {
  const caches = Array.from({ length: 205 }, (_, i) => cache(i + 1, `${prefix}old-${i}`));
  caches.push(cache(206));
  const mock = client(caches);
  const paginate = mock.args.github.paginate;
  let pagesListed = 0;
  mock.args.github.paginate = async (route, params) => {
    const snapshot = await paginate(route, params);
    const complete = [];
    for (let i = 0; i < snapshot.length; i += 100) {
      await Promise.resolve();
      assert.equal(mock.deleted.length, 0, 'must not delete while listing pages');
      complete.push(...snapshot.slice(i, i + 100));
      pagesListed++;
    }
    return complete;
  };
  await prune(mock.args);
  assert.equal(pagesListed, 3);
  assert.deepEqual(mock.listed, {
    owner: 'titan-engine', repo: 'titan', ref: 'refs/heads/main', key: prefix, per_page: 100,
  });
  assert.equal(mock.deleted.length, 205);
  assert.deepEqual(mock.deleted.at(-1), { owner: 'titan-engine', repo: 'titan', cache_id: 1 });
  assert.equal(mock.warnings.length, 0);
});

test('save warnings / API visibility delays do not lead to deletion', async () => {
  const mock = client([cache(1, `${prefix}old`)]);
  await prune(mock.args);
  assert.deepEqual(mock.deleted, []);
  assert.equal(mock.warnings.length, 1);
});

test('visibility delays are retried before pruning, with a bounded wait budget', async () => {
  const old = cache(1, `${prefix}old`);
  const mock = client([old, cache(2)]);
  const paginate = mock.args.github.paginate;
  let attempts = 0;
  const waits = [];
  mock.args.github.paginate = async (route, params) => ++attempts < 3 ? [old] : paginate(route, params);
  mock.args.sleep = async ms => waits.push(ms);
  await prune(mock.args);
  assert.equal(attempts, 3);
  assert.deepEqual(waits, [2000, 2000]);
  assert.deepEqual(mock.deleted, [{ owner: 'titan-engine', repo: 'titan', cache_id: 1 }]);

  attempts = 0;
  mock.deleted.length = 0;
  mock.args.github.paginate = async () => { attempts++; return [old]; };
  await prune(mock.args);
  assert.equal(attempts, 3);
  assert.deepEqual(mock.deleted, []);
});

test('non-main invocations fail before calling the API', async () => {
  const mock = client([cache(1), cache(2)]);
  mock.args.context.ref = 'refs/heads/feature';
  await assert.rejects(prune(mock.args), /restricted to refs\/heads\/main/);
  assert.equal(mock.listed, undefined);
  assert.deepEqual(mock.deleted, []);
});

test('listing and deletion errors fail the cleanup instead of hiding permission problems', async () => {
  const mock = client([cache(1), cache(2)]);
  mock.args.github.paginate = async () => { throw new Error('list denied'); };
  await assert.rejects(prune(mock.args), /list denied/);
  assert.deepEqual(mock.deleted, []);
  mock.args.github.paginate = async () => [cache(1), cache(2)];
  mock.args.github.rest.actions.deleteActionsCacheById = async () => { throw new Error('delete denied'); };
  await assert.rejects(prune(mock.args), /delete denied/);
});
