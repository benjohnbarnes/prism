//! Probe: does `apply_semantic_patch` allow changing a definition's visibility?
//! Widening (private -> public) should be harmless; narrowing (public -> private)
//! should break dependents and be refused. Both are same-name/same-kind/same-body.
//!
//! Usage: cargo rustc --example agent_patch_vis --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::patch::{PatchArtifact, SurfaceTerm};
use prism::{apply_semantic_patch, default_roots, fetch_semantic_patch, with_prelude};
use std::path::Path;

fn main() {
    let roots = default_roots(Path::new("."));

    // (1) Widening: private -> public, same body.
    let entry = "fn f(x : Int) : Int = x\nfn main() = println(f(1))\n";
    let full = with_prelude(entry);
    let fetched = fetch_semantic_patch(entry, &full, &roots, "f").unwrap();
    println!("[before] f rendered: {}", fetched.rendered.trim());
    let replacement = SurfaceTerm::from_source("pub fn f(x : Int) : Int = x").unwrap();
    let patch = PatchArtifact::new(fetched.namespace, fetched.target, replacement, None).unwrap();
    match apply_semantic_patch(entry, &full, &roots, &patch) {
        Ok((d, src)) => {
            println!("WIDEN  (private->pub) ACCEPTED: tier {} core_after {}",
                d.tier.level, d.core_hash_after);
            println!("        result source: {}", src.replace('\n', " ").trim());
        }
        Err(e) => println!("WIDEN  (private->pub) REFUSED: {}", e.message),
    }

    // (2) Narrowing: public -> private, same body.
    let entry2 = "pub fn g(x : Int) : Int = x\nfn main() = println(g(1))\n";
    let full2 = with_prelude(entry2);
    let fetched2 = fetch_semantic_patch(entry2, &full2, &roots, "g").unwrap();
    println!("[before] g rendered: {}", fetched2.rendered.trim());
    let replacement2 = SurfaceTerm::from_source("fn g(x : Int) : Int = x").unwrap();
    let patch2 = PatchArtifact::new(fetched2.namespace, fetched2.target, replacement2, None).unwrap();
    match apply_semantic_patch(entry2, &full2, &roots, &patch2) {
        Ok((d, _)) => println!("NARROW (pub->private) ACCEPTED: tier {}", d.tier.level),
        Err(e) => println!("NARROW (pub->private) REFUSED: {}", e.message),
    }
}
