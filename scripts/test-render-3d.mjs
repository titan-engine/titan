// Build and execute the same headless public API fixture as native tests.
// Execution runs in a bounded child so a WASM trap or hang cannot stall CI.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { execFile } from './acceptance_process.mjs';

if (process.argv[2] === '--execute') {
  const { instance } = await WebAssembly.instantiate(await readFile(process.argv[3]), {});
  assert.equal(instance.exports.verify_render_3d(), 43);
  console.log('Actual WASM 3D mesh, camera, lifecycle and immutable extraction verified.');
} else {
  const repo = fileURLToPath(new URL('../', import.meta.url));
  const options = { cwd: repo, encoding: 'utf8', phase: 'build' };
  const metadata = JSON.parse(await execFile('cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1'], options));
  await execFile('cargo', ['build', '--locked', '-p', 'titan', '--example', 'render_3d', '--no-default-features', '--target', 'wasm32-unknown-unknown'], options);
  const wasm = resolve(metadata.target_directory, 'wasm32-unknown-unknown/debug/examples/render_3d.wasm');
  process.stdout.write(await execFile(process.execPath, [fileURLToPath(import.meta.url), '--execute', wasm], { cwd: repo, encoding: 'utf8' }));
}
