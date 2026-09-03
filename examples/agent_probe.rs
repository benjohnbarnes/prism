//! Spike probe: type-check and interpret Prism source through the embed API,
//! printing each declaration's inferred type and effect row (the static sandbox
//! surface) and the captured program output.
//!
//! Usage: cargo run --example agent_probe --no-default-features -- FILE.pr [...]

use prism::{check, interpret, with_prelude};
use std::collections::HashSet;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: agent_probe FILE.pr [...]");
        std::process::exit(2);
    }

    // Names the prelude (and the stdlib modules it imports) introduces, so we
    // print only the corpus file's own declarations.
    let prelude_names: HashSet<String> = match check(&with_prelude("")) {
        Ok(c) => c.defs.decls.iter().map(|d| d.name.clone()).collect(),
        Err(_) => HashSet::new(),
    };

    for path in &args {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("read {path}: {e}");
                continue;
            }
        };
        let full = with_prelude(&src);
        println!("== {path} ==");

        match check(&full) {
            Ok(checked) => {
                println!("[check] OK");
                for d in &checked.defs.decls {
                    if prelude_names.contains(&d.name) {
                        continue;
                    }
                    let pure = if d.pure { "  (pure)" } else { "" };
                    // `Type::show` already renders the effect row when non-empty,
                    // e.g. "() -> Int ! {Random}".
                    println!("  {} : {}{}", d.name, d.ty.show(), pure);
                }
            }
            Err(e) => {
                println!("[check] FAILED");
                println!("{e}");
            }
        }

        match interpret(&full) {
            Ok(run) => {
                println!("[interpret] OK ({} steps)", run.steps);
                let t = run.term.trim_end();
                if !t.is_empty() {
                    println!("{t}");
                }
            }
            Err(e) => println!("[interpret] FAILED: {e}"),
        }
        println!();
    }
}
