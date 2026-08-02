//! #3b hash-stability probe (native front-end).
//!
//! A native counterpart to the wasm `hash_defs` / `run` exports, for the workbook's
//! hash-stability check (`workbook/docs/NEXT-PIECES.md` §3b). `wasm.rs` is compiled
//! only under `--features wasm`, so its exports are unreachable in a native build;
//! this reproduces the two we need — the content-hash read-out and the run value —
//! over the same unconditional front-end functions (`dump("namespace")`,
//! `interpret`), so native output can be diffed byte-for-byte against the wasm
//! output on identical programs. Float transcendentals are the case that can
//! diverge (native FFIs the vendored C `libm`; wasm uses the pure-Rust `libm`).
//!
//! Build/run with the front-end only (no LLVM):
//!   cargo run --example hash_probe --no-default-features < program.pr
//!
//! Reads a Prism program on stdin, prints `{ "hashes": [...], "value": "..." }`.

use std::collections::{HashMap, HashSet};
use std::io::Read;

use prism::core::HASH_PREFIX_HEX;
use prism::{dump, example_program, interpret, with_prelude};
use serde_json::Value;

/// A faithful reproduction of `wasm.rs::hash_defs`, over the native front-end.
/// Kept structurally identical so the two cannot silently drift.
fn hash_defs(src: &str) -> String {
    let err = |m: &str| serde_json::json!({ "error": m }).to_string();
    let namespace = |full: &str| -> Result<Value, String> {
        let ns = dump("namespace", full).map_err(|e| format!("{e}"))?;
        serde_json::from_str::<Value>(&ns).map_err(|_| "could not read namespace export".to_string())
    };
    let names_of = |doc: &Value| -> Vec<String> {
        doc.get("defs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|d| d.pointer("/meta/name").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let prelude: HashSet<String> = match namespace(&with_prelude("")) {
        Ok(v) => names_of(&v).into_iter().collect(),
        Err(e) => return err(&e),
    };
    let doc = match namespace(&with_prelude(src)) {
        Ok(v) => v,
        Err(e) => return err(&e),
    };
    let Some(defs) = doc.get("defs").and_then(Value::as_array) else {
        return err("namespace export had no defs");
    };
    let name_by_hash: HashMap<&str, &str> = defs
        .iter()
        .filter_map(|d| Some((d.get("hash")?.as_str()?, d.pointer("/meta/name")?.as_str()?)))
        .collect();
    let mut out: Vec<Value> = Vec::new();
    for d in defs {
        let name = match d.pointer("/meta/name").and_then(Value::as_str) {
            Some(n) if !prelude.contains(n) => n,
            _ => continue,
        };
        let hash = d.get("hash").and_then(Value::as_str).unwrap_or_default();
        let short = &hash[..hash.len().min(HASH_PREFIX_HEX)];
        let mut dep_names: Vec<&str> = d
            .pointer("/anon/deps")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter_map(|h| name_by_hash.get(h).copied())
                    .filter(|n| !prelude.contains(*n))
                    .collect()
            })
            .unwrap_or_default();
        dep_names.sort_unstable();
        dep_names.dedup();
        out.push(serde_json::json!({ "name": name, "hash": short, "deps": dep_names }));
    }
    Value::Array(out).to_string()
}

/// Mirror of `wasm.rs::run` for the comparison (the browser-only off-platform-IO
/// guard is omitted — the probe's programs are pure).
fn run(src: &str) -> String {
    let program = example_program(src);
    let wrapped = program != src;
    let full = with_prelude(&program);
    match interpret(&full) {
        Ok(r) => {
            if wrapped {
                let v = r.value.show();
                if r.term.is_empty() {
                    format!("=> {v}")
                } else {
                    format!("{}\n=> {v}", r.term.trim_end_matches('\n'))
                }
            } else {
                r.term
            }
        }
        Err(e) => format!("error: {e}"),
    }
}

fn probe(src: &str) -> Value {
    let hashes: Value = serde_json::from_str(&hash_defs(src)).unwrap_or(Value::Null);
    serde_json::json!({ "hashes": hashes, "value": run(src) })
}

fn main() {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .expect("read stdin");
    // Batch mode: stdin is a JSON array of programs → a JSON array of results (one
    // process, so the front-end starts once). Otherwise a single program → one result.
    if let Ok(Value::Array(progs)) = serde_json::from_str::<Value>(&input) {
        let out: Vec<Value> = progs
            .iter()
            .map(|p| probe(p.as_str().unwrap_or("")))
            .collect();
        println!("{}", Value::Array(out));
    } else {
        println!("{}", probe(&input));
    }
}
