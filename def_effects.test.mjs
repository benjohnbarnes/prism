// Acceptance test for `def_effects` — the per-definition effect row.
// Run AFTER building the patched wasm (`mise run wasm`):  node def_effects.test.mjs
//
// The point of the export: a caller can tell, from the *type*, which host
// operations a definition needs, before running it. That is what lets a host gate
// execution on a capability surface, and lets the workbook tell an effectful cell
// from a pure one without asking the author to assert it.

import init, { def_effects } from './web/pkg/prism.js';
import { readFileSync } from 'node:fs';

await init(readFileSync(new URL('./web/pkg/prism_bg.wasm', import.meta.url)));

let failures = 0;
const ok = (cond, msg) => {
  console.log(`${cond ? 'ok  ' : 'FAIL'}  ${msg}`);
  if (!cond) failures++;
};

const rows = (src) => {
  const out = JSON.parse(def_effects(src));
  if (!Array.isArray(out)) throw new Error('def_effects error: ' + JSON.stringify(out));
  return Object.fromEntries(out.map((r) => [r.name, r]));
};

// ── A. A pure definition has an empty row; the prelude is elided ──────────────
{
  const r = rows(`fn a() = 10\nfn b() = a() + 1`);
  ok(Object.keys(r).length === 2, `A: only the snippet's own defs (got ${Object.keys(r)})`);
  ok(r.a.effects.length === 0, 'A: a is pure');
  ok(r.a.row === '{}', `A: a renders as {} (got ${r.a.row})`);
}

// ── B. A host effect appears in the row of the definition that performs it ───
{
  const src = `
effect Http
  get_url(String) : String

fn pure_one() = 1
fn fetches() = get_url("https://example.com")
`;
  const r = rows(src);
  ok(r.fetches.effects.includes('Http'), `B: fetches names Http (row ${r.fetches.row})`);
  ok(!r.pure_one.effects.includes('Http'), 'B: a pure sibling does not');
}

// ── C. The row propagates to callers — the capability is transitive ───────────
{
  const src = `
effect Http
  get_url(String) : String

fn fetches() = get_url("https://example.com")
fn uses_it() = fetches()
fn unrelated() = 2 + 2
`;
  const r = rows(src);
  ok(r.uses_it.effects.includes('Http'), `C: a caller inherits Http (row ${r.uses_it.row})`);
  ok(r.unrelated.effects.length === 0, 'C: an unrelated def stays pure');
}

// ── D. The workbook's actual question: is this cell effectful? ────────────────
// A cell is effectful iff its row contains a host effect — its row minus IO.
{
  const src = `
effect Http
  get_url(String) : String

fn cell_a() = 41 + 1
fn cell_b() = get_url("https://example.com")
fn cell_c() = println("hi")
`;
  const r = rows(src);
  const hostEffects = (row) => row.effects.filter((e) => e !== 'IO');
  ok(hostEffects(r.cell_a).length === 0, 'D: a pure cell needs no capability');
  ok(
    hostEffects(r.cell_b).join() === 'Http',
    `D: an effect cell names exactly its capability (got ${hostEffects(r.cell_b)})`,
  );
  ok(hostEffects(r.cell_c).length === 0, `D: IO alone is not a host capability (row ${r.cell_c.row})`);
}

// ── E. A broken program reports an error rather than lying ───────────────────
{
  const out = JSON.parse(def_effects('fn a() = 3 + "x"'));
  ok(!Array.isArray(out) && typeof out.error === 'string', 'E: a type error returns {error}');
}

console.log(failures ? `\n${failures} FAILED` : '\nall passed ✓');
process.exit(failures ? 1 : 0);
