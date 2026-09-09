//! Probe: Prism's reflection surface for a host — `module_interface` + `check_on`
//! give public types, values, instance conformance, effect rows, and shape
//! digests, without any codegen round-trip.
//!
//! Usage: cargo rustc --example agent_introspect --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::{check_on, default_roots, module_interface, show_effects, with_prelude};
use std::path::Path;

fn main() {
    let entry = "import Json (..)\n\n\
        pub type Req = Req { n : Int } deriving (Show, Eq, ToJson, FromJson)\n\
        pub type Resp = Resp { n2 : Int } deriving (Show, Eq, ToJson, FromJson)\n\n\
        pub fn tool(req : Req) : Resp = Resp { n2 = req.n * 2 }\n";

    let full = with_prelude(entry);
    let roots = default_roots(Path::new("."));

    println!("=== module_interface entries (non-dependency) ===");
    let iface = module_interface(entry, &full, &roots).unwrap();
    for e in &iface.entries {
        if e.kind.starts_with("dependency") {
            continue;
        }
        println!("[{}] name={} sig={}", e.kind, e.name, e.signature);
    }

    println!();
    println!("=== check_on decls (top-level, no dot) ===");
    let checked = check_on(&full, &roots).unwrap();
    for d in &checked.defs.decls {
        if d.name.contains('.') || d.name.contains('@') {
            continue;
        }
        println!(
            "{} : {} ! {} pure={}",
            d.name,
            d.ty.show(),
            show_effects(&d.effects),
            d.pure
        );
    }
}
