// #3b — native-vs-wasm hash-stability & value-determinism differential.
//
// Runs the same Prism programs through the **native** front-end (the
// `hash_probe` example, built `--no-default-features`, no LLVM) and the **wasm**
// build (the workbook's vendored bundle), and diffs:
//   • content HASHES (via `hash_defs`) — must be identical (hashing is over the
//     definition, not the runtime value); this underpins cutoff/caching/staleness.
//   • run VALUES — float transcendentals are the case that can diverge, ~1 ULP,
//     between native (vendored C libm via FFI) and wasm (pure-Rust `libm`).
//
// Prereqs:
//   (prism/) cargo build --release --example hash_probe --no-default-features
//   node hash_stability.mjs
//
// Exit 0 iff hashes are identical (the required property). Value divergences are
// reported but do not fail the run: they perturb no hash and — per the workbook's
// architecture — never cross the platform boundary (the content-addressed value
// store is local cache; only the CRDT, holding sources + pinned *effect* results
// as stored bytes, is synced). See docs/NEXT-PIECES.md §3b and PROGRESS.md.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const vendor = join(here, "../workbook/vendor");
const { default: init, run, hash_defs } = await import(join(vendor, "prism.mjs"));
await init(readFileSync(join(vendor, "prism_bg.wasm")));

const BIN = join(here, "target/release/examples/hash_probe");
const nativeBatch = (progs) =>
  JSON.parse(execFileSync(BIN, { input: JSON.stringify(progs), encoding: "utf8", maxBuffer: 64 << 20 }));

const unary = ["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh",
  "exp", "exp2", "expm1", "log", "log2", "log10", "log1p", "cbrt", "sqrt"];
// A representative spread (includes the known ~1-ULP-divergent inputs). The full
// 20-point sweep that first found them is in git history / PROGRESS.md; this keeps
// the reproducible run quick (each wasm `run` is ~215 ms).
const args = [0.5, 0.9, 1.0, 1.3, 2.0, 2.5, 5.0, 10.0, 0.001];
const binary = ["pow", "atan2", "hypot", "fmod"];
const pairs = [[2, 0.5], [2, 10], [3, 3], [10, 0.1], [1.5, 2.5], [7, 3], [0.5, 0.5], [100, 0.25]];

const valueExprs = [];
for (const f of unary) for (const a of args) valueExprs.push(`${f}(${a})`);
for (const f of binary) for (const [a, b] of pairs) valueExprs.push(`${f}(${a}, ${b})`);
valueExprs.push("sin(1.0) +. cos(1.0)", "exp(log(2.0))", "pow(sin(0.5), 2.0) +. pow(cos(0.5), 2.0)");

const hashProgs = [
  "fn a() = 1\nfn b() = a() + 2\nfn c() = b() * a()",
  "fn x() = sin(1.0)\nfn y() = x() +. cos(1.0)",
  "fn s() = sqrt(2.0)\nfn t() = s() *. s()",
  'fn g() = "hello"\nfn h() = str_len(g())',
];

const nat = nativeBatch([...valueExprs, ...hashProgs]);

let valMismatch = 0, valErr = 0;
const diffs = [];
valueExprs.forEach((e, i) => {
  const w = String(run(e)).trim();
  const n = String(nat[i].value).trim();
  if (/error/i.test(w) || /error/i.test(n)) { valErr++; if (w !== n) { valMismatch++; diffs.push([e, w, n]); } return; }
  if (w !== n) { valMismatch++; diffs.push([e, w, n]); }
});

let hashMismatch = 0;
const hdiffs = [];
hashProgs.forEach((p, j) => {
  const w = hash_defs(p);
  const n = JSON.stringify(nat[valueExprs.length + j].hashes);
  if (w !== n) { hashMismatch++; hdiffs.push([p, w, n]); }
});

console.log(`VALUES: ${valueExprs.length} checked, ${valErr} error-results, ${valMismatch} divergent (~1 ULP transcendentals)`);
for (const [e, w, n] of diffs) console.log(`  ${e}\n    wasm:   ${w}\n    native: ${n}`);
console.log(`HASHES: ${hashProgs.length} programs, ${hashMismatch} MISMATCH`);
for (const [p, w, n] of hdiffs) console.log(`  ${JSON.stringify(p)}\n    wasm:   ${w}\n    native: ${n}`);

if (hashMismatch === 0) {
  console.log(`\n✓ HASH STABILITY HOLDS (native == wasm on every def). Value divergences: ${valMismatch} (perturb no hash; inert in the workbook architecture).`);
  process.exit(0);
} else {
  console.error(`\n✗ HASH DIVERGENCE — ${hashMismatch} program(s) hash differently native vs wasm. This breaks cutoff/caching and must be fixed.`);
  process.exit(1);
}
