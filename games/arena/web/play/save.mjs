export const MAX_SAVE_BYTES = 64 * 1024;

// Files stay local. Validate transport bounds here and gameplay/schema in Rust.
export async function readSaveForSession(file, session, currentSession) {
  if (!session || !session.paused() || !session.control_enabled()) {
    throw new Error('Pause the game and enable inspection controls before loading.');
  }
  if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_SAVE_BYTES) {
    throw new Error('Save file exceeds the 64 KiB limit.');
  }
  const epoch = session.clock_epoch();
  const text = await file.text();
  if (new TextEncoder().encode(text).byteLength > MAX_SAVE_BYTES) {
    throw new Error('Save file exceeds the 64 KiB limit.');
  }
  const save = JSON.parse(text);
  if (currentSession() !== session || session.clock_epoch() !== epoch
      || !session.paused() || !session.control_enabled()) {
    throw new Error('The game or its controls changed while reading. Choose the save again.');
  }
  return save;
}
