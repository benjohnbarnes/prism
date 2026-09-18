//! Probe: calling a class method that lives in a *module*, from the host.
//!
//! `agent_render.rs` answered this for a class declared in the same source, so
//! `render` was simply in scope. The real seam is different: the class is
//! declared in `UI.pr` — a module — and the instance lives in the caller's code.
//! Two questions, each of which changes a line of host code:
//!
//!   1. How is the method called? Bare `render(v, i)`, qualified `UI.render(v, i)`,
//!      or must the instance be named explicitly with `using`?
//!   2. What exact `class` string does the metadata report, so the host can ask
//!      `conforms_to(type_name, <that string>)`?
//!
//! This is the fork's probe convention (see `agent_render.rs`): an example, not a
//! change to the language. No `prism/` source is modified.
//!
//! Usage: cargo rustc --example agent_ui_seam_call --no-default-features -- -C link-arg=.../libprism_libm.a
//!        ./target/debug/examples/agent_ui_seam_call [module-dir]

use prism::eval::{HostEffect, Rv};
use prism::eval_session::EvalSession;
use prism::{default_roots, host_ops_in, module_interface, show_effects, with_prelude, Root, Sym};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

struct NullHost;

impl HostEffect for NullHost {
    fn call(&mut self, op: Sym, _args: &[Rv]) -> Result<Rv, String> {
        Err(format!("no host op `{op}`"))
    }
}

/// The entry source: the module imported, a local type, and an instance for the
/// imported class. Deliberately *not* a module itself — this is the shape the
/// host evaluates, and the shape every tool that supplies a rendering will have.
const SRC: &str = r#"
import UI (..)

pub type Thing = Thing { n: Int }

instance renderThing : Render(Thing)
  fn render(t, i) = Some(Card { title = Some("thing"), children = [Text("hi")] })
"#;

fn main() {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "../agent-host/assets/prism".to_string()),
    );

    let full = with_prelude(SRC);
    let mut roots = default_roots(Path::new("."));
    roots.push(Root::Dir(dir.clone()));

    println!("=== 1. which class string does the metadata report? ===");
    println!("(the host keys `conforms_to` on exactly this, so it has to be right)");
    match module_interface(SRC, &full, &roots) {
        Ok(iface) => {
            let mut any = false;
            for e in &iface.entries {
                if e.kind.contains("class") || e.kind.contains("instance") {
                    println!("[{}] name={} sig={}", e.kind, e.name, e.signature);
                    any = true;
                }
            }
            if !any {
                println!("(no class/instance entries in the interface)");
            }
        }
        Err(e) => println!("interface ERR: {e}"),
    }

    let session = match EvalSession::build(&full, &roots) {
        Ok(s) => s,
        Err(e) => {
            println!("session build failed: {e}");
            return;
        }
    };
    let ops: HashSet<Sym> = host_ops_in(SRC);
    let mut host = NullHost;

    let value = r#"Thing { n = 1 }"#;
    let intent = r#"Intent { slot = CardSlot, purpose = None }"#;

    println!();
    println!("=== 2. call forms (value local, class in module `UI`) ===");
    let attempts = [
        ("bare*", format!("render({value}, {intent})")),
        ("qualified", format!("UI.render({value}, {intent})")),
        (
            "using-arg",
            format!("render({value}, {intent}, using renderThing)"),
        ),
        (
            "using-arg-qualified",
            format!("UI.render({value}, {intent}, using renderThing)"),
        ),
    ];
    for (label, expr) in &attempts {
        match session.eval(expr, &ops, &mut host) {
            Ok((v, ty, eff)) => println!(
                "[{label:20}] OK   = {} : {} ! {}",
                v.show(),
                ty.show(),
                show_effects(&eff)
            ),
            Err(e) => {
                let first = e.to_string().lines().next().unwrap_or("").to_string();
                println!("[{label:20}] ERR  {first}");
            }
        }
        if *label == "bare*" {
            if let Ok((v, _, _)) = session.eval(expr, &ops, &mut host) {
                println!("  structural form (what the host walker must match): {v:?}");
            }
        }
    }
    println!();
    println!("(* the form the seam should prefer, since it needs no instance name)");
}
