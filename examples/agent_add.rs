//! Probe: can we ADD a brand-new symbol to the content-addressed codebase?
//! (patch is replace-only; the add path is `commit_to_store`.)
//!
//! Usage: cargo rustc --example agent_add --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::{commit_to_store, default_roots, with_prelude, Config};
use prism::store::disk::Store;
use std::path::{Path, PathBuf};

fn main() {
    let store_dir = "/tmp/prism-agent-add-test";
    let _ = std::fs::remove_dir_all(store_dir);

    let src = with_prelude("fn brandnew(a, b) = a * b\nfn main() = println(brandnew(6, 7))");
    let roots = default_roots(Path::new("."));
    let mut cfg = Config::from_env();
    cfg.update_flags(|f| f.store_path = Some(PathBuf::from(store_dir)));

    // ADD: commit the program (which introduces the new `brandnew` symbol).
    let stats = commit_to_store(&src, &roots, &cfg).unwrap();
    println!(
        "commit: {} unchanged, {} recompiled",
        stats.objects_hit, stats.objects_written
    );

    // Query: is the new symbol now registered by name in the store namespace?
    let store = Store::open_or_create(&PathBuf::from(store_dir)).unwrap();
    match store.lookup_name("brandnew") {
        Ok(Some(hash)) => println!("lookup `brandnew` -> {hash}"),
        Ok(None) => println!("`brandnew` NOT registered"),
        Err(e) => println!("lookup error: {e}"),
    }
}
