// Effect-trap spike acceptance test.
// Run AFTER building the patched wasm (`./build-wasm.sh` or `mise run wasm`), from the
// Prism repo root with this file + spike_host.pr alongside:  node spike_host.test.mjs
//
// It loads the freshly built web/pkg wasm, performs an unhandled `Host.host_call` from
// Prism, services it with a JS host that upper-cases, and checks the program resumes.

import init, { run_with_host } from './web/pkg/prism.js';
import { readFileSync } from 'node:fs';

await init(readFileSync(new URL('./web/pkg/prism_bg.wasm', import.meta.url)));

const src = readFileSync(new URL('./spike_host.pr', import.meta.url), 'utf8');

// Host services one op: (op: string, args: string[]) -> string
const host = (op, args) => {
  if (op === 'host_call') return String(args[0]).toUpperCase();
  throw new Error('unknown host op: ' + op);
};

const out = run_with_host(src, host).trim();
console.log('transcript:', JSON.stringify(out));

if (out === 'HI') {
  console.log('HI ✓  — effect-trap spike PASSED (host serviced the op and the program resumed)');
  process.exit(0);
} else {
  console.error('FAILED — expected "HI", got:', JSON.stringify(out));
  process.exit(1);
}
