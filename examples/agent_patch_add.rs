//! Probe: can a semantic patch introduce NOVEL code (a new name), or only
//! replace an existing definition?
//!
//! Usage: cargo rustc --example agent_patch_add --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::patch::{PatchArtifact, SurfaceTerm};
use prism::{apply_semantic_patch, default_roots, fetch_semantic_patch, with_prelude};
use std::path::Path;

fn main() {
    let entry = "fn add(a, b) = a + b\nfn main() = println(add(2, 3))\n";
    let full = with_prelude(entry);
    let roots = default_roots(Path::new("."));

    // (1) Fetch a name that does NOT exist -> patch cannot target a new symbol.
    println!("== fetch a brand-new name ==");
    match fetch_semantic_patch(entry, &full, &roots, "brandnew") {
        Ok(f) => println!("UNEXPECTED OK: {} -> {}", f.name, f.core_hash),
        Err(e) => println!("REFUSED: {}", e.message),
    }

    let fetched = fetch_semantic_patch(entry, &full, &roots, "add").unwrap();

    // (2) Replace `add` with a differently-NAMED definition -> name change.
    println!("== replace `add` with a renamed definition ==");
    let rename = SurfaceTerm::from_source("fn newthing(a, b) = a * b").unwrap();
    let patch = PatchArtifact::new(fetched.namespace, fetched.target, rename, None).unwrap();
    match apply_semantic_patch(entry, &full, &roots, &patch) {
        Ok((d, _)) => println!("UNEXPECTED OK: tier {}", d.tier.level),
        Err(e) => println!("REFUSED: {}", e.message),
    }

    // (3) Replace `add` with a differently-KINDED declaration -> kind change.
    println!("== replace `add` with a `type` declaration ==");
    let fetched2 = fetch_semantic_patch(entry, &full, &roots, "add").unwrap();
    let kind = SurfaceTerm::from_source("type T = T(Int)").unwrap();
    let patch2 = PatchArtifact::new(fetched2.namespace, fetched2.target, kind, None).unwrap();
    match apply_semantic_patch(entry, &full, &roots, &patch2) {
        Ok((d, _)) => println!("UNEXPECTED OK: tier {}", d.tier.level),
        Err(e) => println!("REFUSED: {}", e.message),
    }
}
