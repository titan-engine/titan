// Compile the same isolated scenario runner for native and actual WebAssembly.
// Fixtures are absent from normal builds; no gameplay mutation API is introduced.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from '../../../scripts/acceptance_process.mjs';

const script = fileURLToPath(import.meta.url);
const root = resolve(dirname(script), '..');
if (process.argv.includes('--wasm-worker')) {
  const { puzzle_acceptance } = createRequire(import.meta.url)(process.argv[3]);
  process.stdout.write(puzzle_acceptance());
} else {
  const build = (file, args) => execFile(file, args, { phase: 'build', cwd: root, encoding: 'utf8' });
  const metadata = JSON.parse(await build('cargo', ['metadata', '--locked', '--format-version', '1', '--filter-platform', 'wasm32-unknown-unknown']));
  const target = metadata.target_directory;
  const engine = metadata.packages.find(p => p.name === 'titan');
  const version = metadata.packages.find(p => p.name === 'wasm-bindgen').version;
  const toolRoot = join(target, 'titan/tools');
  const candidates = [join(toolRoot, 'bin/wasm-bindgen'),
    resolve(dirname(engine.manifest_path), 'target/titan/tools/bin/wasm-bindgen'), 'wasm-bindgen'];
  let bindgen;
  for (const candidate of candidates) {
    try {
      if ((await build(candidate, ['--version'])).trim() === `wasm-bindgen ${version}`) {
        bindgen = candidate;
        break;
      }
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  if (!bindgen) {
    await build('cargo', ['install', 'wasm-bindgen-cli', '--version', version, '--locked', '--root', toolRoot, '--force']);
    bindgen = join(toolRoot, 'bin/wasm-bindgen');
  }
  await build('cargo', ['build', '--locked', '--bin', 'puzzle-acceptance', '--features', 'movement-acceptance']);
  const native = JSON.parse(await execFile(join(target, 'debug/puzzle-acceptance'), [], { cwd: root, encoding: 'utf8' }));
  await build('cargo', ['build', '--locked', '--lib', '--target', 'wasm32-unknown-unknown', '--release', '--features', 'movement-acceptance']);
  const directory = await mkdtemp(resolve(tmpdir(), 'adventure-puzzle-'));
  try {
    await build(bindgen, [join(target, 'wasm32-unknown-unknown/release/titan_adventure.wasm'),
      '--target', 'nodejs', '--out-dir', directory, '--out-name', 'movement']);
    const wasm = JSON.parse(await execFile(process.execPath, [script, '--wasm-worker', join(directory, 'movement.js')],
      { cwd: root, encoding: 'utf8' }));
    assert.deepEqual(wasm, native, 'every puzzle fixture tick agrees between native and actual WASM');
    const ticks = Object.values(native.scenarios).reduce((sum, trace) => sum + trace.length, 0);
    console.log(`Adventure puzzle: ${Object.keys(native.scenarios).length} scenarios, ${ticks} states; all assertions and exact native/actual-WASM agreement passed.`);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}
