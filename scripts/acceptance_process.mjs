// Acceptance subprocess deadlines include descendants holding inherited pipes.
// POSIX process groups match the supported Linux/macOS acceptance hosts.
import { spawn } from 'node:child_process';

export function timeoutSeconds(phase = 'runtime') {
  if (!['runtime', 'build'].includes(phase)) throw new Error(`unknown acceptance phase: ${phase}`);
  const key = `TITAN_${phase.toUpperCase()}_TIMEOUT_SECONDS`;
  const seconds = Number(process.env[key] ?? (phase === 'build' ? 1200 : 60));
  if (!Number.isFinite(seconds) || seconds <= 0) throw new Error(`${key} must be finite and positive`);
  return seconds;
}

const owned = new Set();
function terminate(child) {
  if (!child.pid) return;
  try { process.kill(-child.pid, 'SIGKILL'); }
  catch (error) { if (error.code !== 'ESRCH') throw error; }
}
process.once('exit', () => { for (const child of owned) terminate(child); });
for (const [signal, code] of [['SIGINT', 130], ['SIGTERM', 143]]) {
  process.once(signal, () => {
    for (const child of owned) terminate(child);
    process.exit(code);
  });
}

export async function run(file, args = [], options = {}) {
  const { phase = 'runtime', timeout: requestedTimeout = timeoutSeconds(phase) * 1000,
    encoding, stdio = 'pipe', ...spawnOptions } = options;
  let timeout = requestedTimeout;
  const inherited = process.env.TITAN_ACCEPTANCE_DEADLINE_EPOCH;
  if (inherited !== undefined) {
    const deadline = Number(inherited);
    if (!Number.isFinite(deadline) || deadline <= 0) throw new Error('TITAN_ACCEPTANCE_DEADLINE_EPOCH must be finite and positive');
    timeout = Math.min(timeout, deadline * 1000 - Date.now());
  }
  if (!Number.isFinite(timeout) || timeout <= 0) throw new Error('acceptance timeout must be finite and positive');
  return new Promise((resolve, reject) => {
    const env = { ...(spawnOptions.env ?? process.env),
      TITAN_ACCEPTANCE_DEADLINE_EPOCH: String((Date.now() + timeout - Math.min(5000, timeout / 2)) / 1000) };
    const child = spawn(file, args, { ...spawnOptions, env, stdio, detached: true });
    owned.add(child);
    const stdout = [], stderr = [];
    child.stdout?.on('data', chunk => stdout.push(chunk));
    child.stderr?.on('data', chunk => stderr.push(chunk));
    child.stdin?.end();
    let settled = false;
    const result = () => ({
      status: child.exitCode, signal: child.signalCode,
      stdout: child.stdout ? (encoding ? Buffer.concat(stdout).toString(encoding) : Buffer.concat(stdout)) : null,
      stderr: child.stderr ? (encoding ? Buffer.concat(stderr).toString(encoding) : Buffer.concat(stderr)) : null,
    });
    const finish = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      terminate(child);
      owned.delete(child);
      // Never wait for EOF from a pipe a descendant inherited.
      child.stdout?.destroy();
      child.stderr?.destroy();
      if (error) reject(Object.assign(error, result()));
      else resolve(result());
    };
    const timer = setTimeout(() => finish(new Error(`acceptance ${phase} phase timed out after ${timeout / 1000}s: ${file}`)), timeout);
    child.on('error', finish);
    child.on('close', () => finish());
  });
}

export async function execFile(file, args = [], options = {}) {
  const result = await run(file, args, options);
  if (result.status !== 0) {
    throw Object.assign(new Error(`acceptance ${options.phase ?? 'runtime'} command failed (${result.status ?? result.signal}): ${file}`), result);
  }
  return result.stdout;
}
