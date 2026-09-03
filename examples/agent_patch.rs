//! Spike probe: prove "add/replace code at runtime" via the content-addressed
//! semantic-patch API — the same verbs `prism patch serve` exposes over stdio.
//!
//! Usage: cargo rustc --example agent_patch --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::patch::{PatchArtifact, SurfaceTerm};
use prism::{apply_semantic_patch, default_roots, fetch_semantic_patch, interpret, with_prelude};
use std::path::Path;

fn main() {
    let entry = "fn add(a, b) = a + b\nfn main() = println(add(2, 3))\n";
    let full = with_prelude(entry);
    let roots = default_roots(Path::new("."));

    // 1. Original program.
    let r0 = interpret(&full).unwrap();
    println!("[before] add(2,3) => {}", r0.term.trim_end());

    // 2. Fetch the content-addressed identity of `add`.
    let fetched = fetch_semantic_patch(entry, &full, &roots, "add").unwrap();
    println!("[fetch]  core_hash  = {}", fetched.core_hash);
    println!("[fetch]  ty         = {}", fetched.ty);
    println!("[fetch]  effect_row = {}", fetched.effect_row);
    println!("[fetch]  rendered   = {}", fetched.rendered.trim());

    // 3. Author a replacement body (add -> multiply).
    let replacement = SurfaceTerm::from_source("fn add(a, b) = a * b").unwrap();
    let patch = PatchArtifact::new(fetched.namespace, fetched.target, replacement, None).unwrap();

    // 4. Apply: judge the patch against the pinned namespace + digest.
    let (delta, new_source) = apply_semantic_patch(entry, &full, &roots, &patch).unwrap();
    println!(
        "[apply]  tier {} ({})",
        delta.tier.level, delta.tier.claim
    );
    println!(
        "[apply]  core_hash  {} -> {}",
        delta.core_hash_before, delta.core_hash_after
    );

    // 5. Re-run the patched program from the returned canonical source.
    let r1 = interpret(&new_source).unwrap();
    println!("[after]  add(2,3) => {}", r1.term.trim_end());
}
