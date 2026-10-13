// Runs a Titan wasm command module (tests, tools) in Node: node tools/wasm_run.mjs file.wasm
// Implements the host interface declared in src/platform/web/wasi_shim.c.
import { readFile } from "node:fs/promises";

class Exit {
  constructor(code) { this.code = code; }
}

const bytes = await readFile(process.argv[2]);
let memory;
const env = {
  titan_write(fd, ptr, len) {
    const data = new Uint8Array(memory.buffer, ptr, len).slice(); // copy: memory may grow
    (fd === 2 ? process.stderr : process.stdout).write(data);
  },
  titan_now: () => performance.now(),
  titan_exit(code) { throw new Exit(code); },
};

const { instance } = await WebAssembly.instantiate(bytes, { env });
memory = instance.exports.memory;
try {
  instance.exports._start();
} catch (e) {
  if (e instanceof Exit) {
    process.exitCode = e.code;
  } else {
    console.error(e); // trap: abort(), assert, unreachable
    process.exitCode = 134;
  }
}
