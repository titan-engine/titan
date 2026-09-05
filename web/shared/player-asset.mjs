export const PLAYER_PNG_MAX_BYTES = 256 * 1024;

/** Fetch before game construction. Bound both elapsed time and streamed bytes. */
export async function loadPlayerPng({ fetch = globalThis.fetch, url = '../assets/player.png', timeoutMs = 10000, maxBytes = PLAYER_PNG_MAX_BYTES } = {}) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  let reader;
  try {
    const response = await fetch(url, { signal: controller.signal, cache: 'no-store' });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const length = response.headers.get('content-length');
    if (length !== null && Number(length) > maxBytes) throw new Error(`PNG exceeds ${maxBytes} bytes`);
    if (!response.body) throw new Error('response has no readable body');
    reader = response.body.getReader();
    const chunks = [];
    let size = 0;
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > maxBytes) throw new Error(`PNG exceeds ${maxBytes} bytes`);
      chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return bytes;
  } catch (error) {
    throw new Error(`Could not load ${url}: ${controller.signal.aborted ? `timed out after ${timeoutMs} ms` : error.message ?? error}`);
  } finally {
    clearTimeout(timer);
    if (reader) { await reader.cancel().catch(() => {}); reader.releaseLock(); }
    controller.abort();
  }
}

/** Each source has its own byte/time budget. No partial pair escapes to a host. */
export async function loadRpgPngs(options = {}) {
  const [player, tree] = await Promise.all([
    loadPlayerPng({ ...options, url: '../assets/player.png' }),
    loadPlayerPng({ ...options, url: '../assets/tree.png' }),
  ]);
  return { player, tree };
}
