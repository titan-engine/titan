// Called only by the main-branch cache producers, after a save or an exact hit.
const MAIN_REF = 'refs/heads/main';

function cachesToDelete(caches, prefix, currentKey) {
  if (!prefix || !currentKey || !currentKey.startsWith(prefix) || currentKey === prefix) {
    throw new Error('A complete cache key and its nonempty restore prefix are required');
  }

  // Filter again even though the API request is scoped: never delete another
  // platform/toolchain/target, a PR cache, or a cache on another branch.
  const matching = caches.filter(cache =>
    cache.ref === MAIN_REF && cache.key.startsWith(prefix)
  );
  // actions/cache/save can report success after only logging an upload warning.
  // Do not remove the fallback until the replacement is visible in the API.
  if (!matching.some(cache => cache.key === currentKey)) {
    return [];
  }

  // Keys alone do not establish compatibility: paths/compression determine a
  // separate cache version. During a version migration, leave all fallbacks
  // intact rather than deleting the last version an older consumer can restore.
  if (matching.some(cache => !cache.version) || new Set(matching.map(cache => cache.version)).size !== 1) {
    return [];
  }

  // Retain the newest entry, not necessarily this run's key. This also protects
  // a newer cache if an older producer finishes late. Access time is irrelevant.
  matching.sort((a, b) => b.created_at.localeCompare(a.created_at) || b.id - a.id);
  return matching.slice(1);
}

async function prune({ github, context, core, prefix, currentKey, sleep = ms => new Promise(resolve => setTimeout(resolve, ms)) }) {
  if (context.ref !== MAIN_REF) {
    throw new Error('Cache pruning is restricted to refs/heads/main');
  }
  const params = { ...context.repo, ref: MAIN_REF, key: prefix, per_page: 100 };
  // Fetch every page before deleting anything; deletion during pagination can
  // otherwise shift entries onto an already visited page.
  let caches;
  for (let attempt = 0; attempt < 3; attempt++) {
    caches = await github.paginate(github.rest.actions.getActionsCacheList, params);
    if (caches.some(cache => cache.ref === MAIN_REF && cache.key === currentKey)) {
      break;
    }
    // Allow a short visibility delay after upload, but never delete on an
    // unconfirmed save, even after all retries have been exhausted.
    if (attempt < 2) await sleep(2000);
  }
  const obsolete = cachesToDelete(caches, prefix, currentKey);
  const matching = caches.filter(cache => cache.ref === MAIN_REF && cache.key.startsWith(prefix));
  if (!matching.some(cache => cache.key === currentKey)) {
    core.warning(`Cache ${currentKey} is not visible; leaving fallback caches intact`);
    return;
  }
  if (matching.some(cache => !cache.version) || new Set(matching.map(cache => cache.version)).size !== 1) {
    core.warning(`Mixed or unknown cache versions for ${prefix}; leaving fallbacks intact until consumers migrate`);
    return;
  }
  for (const cache of obsolete) {
    core.info(`Deleting obsolete main cache ${cache.id}: ${cache.key}`);
    await github.rest.actions.deleteActionsCacheById({ ...context.repo, cache_id: cache.id });
  }
  core.info(`Pruned ${obsolete.length} obsolete cache(s) for ${prefix}`);
}

module.exports = { cachesToDelete, prune };
