//! Probe: does the Prism mirror of the host's `UITree` grammar load as a module?
//!
//! The mirror (`agent-host/assets/prism/UI.pr`) is a *module*, so it is checked
//! the way the host will load it: a directory `Root` that `import UI (..)`
//! resolves against. Checking it as a standalone entry source would be wrong —
//! the prelude is prepended to the source, which pushes the module's own imports
//! out of the leading position and leaves them unresolved.
//!
//! Usage: cargo rustc --example agent_ui_mirror --no-default-features -- -C link-arg=.../libprism_libm.a
//!        ./target/debug/examples/agent_ui_mirror [module-dir]

use prism::{check_on, default_roots, with_prelude, Root};
use std::path::{Path, PathBuf};

fn main() {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "../agent-host/assets/prism".to_string()),
    );

    // The entry source is what the *host* will run: something that imports the
    // module. If the mirror has a syntax or type error, or refers to a name it
    // has not imported, it surfaces here.
    let entry = "import UI (..)\n\n\
                 fn probe_mirror() : UITree = Card { title = None, children = [Text(\"ok\")] }\n\n\
                 fn probe_intent() : Intent = Intent { slot = CardSlot, purpose = None }\n";

    let mut roots = default_roots(Path::new("."));
    roots.push(Root::Dir(dir.clone()));

    match check_on(&with_prelude(entry), &roots) {
        Ok(_) => println!(
            "ok — {} loads as module `UI`; `import UI (..)` resolves, and its \
             UI.UITree / UI.Intent constructors are usable",
            dir.display()
        ),
        Err(e) => println!("FAILED to load {}:\n{e}", dir.display()),
    }
}
