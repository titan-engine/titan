export const MAX_RECORDING_BYTES = 2 * 1024 * 1024;

export async function readRecordingForSession(file, session, currentSession) {
  if (!session || !session.paused()) throw new Error('Pause the player before loading a recording.');
  if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_RECORDING_BYTES) {
    throw new Error('Recording exceeds the 2 MiB limit.');
  }
  const epoch = session.clock_epoch();
  const text = await file.text();
  if (new TextEncoder().encode(text).byteLength > MAX_RECORDING_BYTES) {
    throw new Error('Recording exceeds the 2 MiB limit.');
  }
  if (currentSession() !== session || session.clock_epoch() !== epoch || !session.paused()) {
    throw new Error('The session changed while reading. Choose the recording again.');
  }
  // The core validates the bounded JSON and recording before installing playback.
  return text;
}
