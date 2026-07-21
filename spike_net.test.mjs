// Effect-bridge piece 2 acceptance test (2a gate + 2b async + 2c real capability).
// Run AFTER building the patched wasm (`mise run wasm`):  node spike_net.test.mjs
//
// It exercises `run_with_async_host`: the Prism run SUSPENDS at an unhandled host
// op, the JS host services it across a real `await` (Promise), and the program
// RESUMES with the result. Two checks:
//   A. deterministic async — proves suspend/await/resume with no network.
//   B. real capability — a Prism program performs `Net.fetch(url)` serviced by the
//      host's real `fetch()`, and prints a known substring of the response.

import init, { run_with_async_host, run_with_host, diagnostics } from './web/pkg/prism.js';
import { readFileSync } from 'node:fs';

await init(readFileSync(new URL('./web/pkg/prism_bg.wasm', import.meta.url)));

const delay = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const ok = (cond, msg) => {
  console.log(`${cond ? 'ok  ' : 'FAIL'}  ${msg}`);
  if (!cond) failures++;
};

// ── A. Deterministic async: the host resumes only after an awaited macrotask ──
{
  const src = `
effect Clock
  now(String) : String

fn main() : Unit ! {IO, Clock} =
  println(now("tick"))
`;
  let hostAwaited = false;
  const host = async (op, args) => {
    ok(op === 'now', `A: host received op '${op}'`);
    await delay(15); // a real async boundary the interpreter must suspend across
    hostAwaited = true;
    return 'PONG';
  };
  const promise = run_with_async_host(src, host);
  ok(promise instanceof Promise, 'A: run_with_async_host returns a Promise');
  ok(!hostAwaited, 'A: run is genuinely suspended (host await not yet resolved)');
  const out = (await promise).trim();
  console.log('A transcript:', JSON.stringify(out));
  ok(out === 'PONG', 'A: program resumed with the awaited host value → PONG');
}

// ── B. Real capability: Net.fetch serviced by the host's real fetch() ──
{
  const src = readFileSync(new URL('./spike_net.pr', import.meta.url), 'utf8');
  ok(diagnostics(src) === '[]', 'B: spike_net.pr type-checks clean');
  const host = async (op, args) => {
    if (op !== 'get_url') throw new Error('unexpected host op: ' + op);
    const res = await fetch(String(args[0]));
    return await res.text();
  };
  try {
    const out = await run_with_async_host(src, host);
    const substr = 'Example Domain';
    console.log('B transcript (first 80):', JSON.stringify(out.slice(0, 80)));
    ok(out.includes(substr), `B: fetched body printed, contains "${substr}"`);
  } catch (e) {
    ok(false, `B: real fetch failed (network?): ${e}`);
  }
}

// ── Gate sanity (2a): the sync spike still passes under the host_ops gate ──
{
  const src = readFileSync(new URL('./spike_host.pr', import.meta.url), 'utf8');
  const out = run_with_host(src, (op, args) => String(args[0]).toUpperCase()).trim();
  ok(out === 'HI', '2a: gated sync run_with_host still services host_call → HI');
}

if (failures === 0) {
  console.log('\n✓ ALL PASSED — effect bridge 2a/2b/2c (async suspend/resume + real fetch)');
  process.exit(0);
} else {
  console.error(`\n✗ ${failures} check(s) FAILED`);
  process.exit(1);
}
