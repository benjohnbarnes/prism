//! Probe: a `Render` type class, from a host's point of view.
//!
//! Four questions, in order:
//!   1. Does `module_interface` report the class and the instance, and under
//!      what names? (The host has to ask for the class by some exact string.)
//!   2. Does a bare method call `render(v, i)` resolve the instance from the
//!      constraint alone, or does it need an explicit `using`?
//!   3. What does the returned `UITree` actually *be* as an `Rv` — can the host
//!      walk it into a Rust enum without going through JSON?
//!   4. What happens when a type has no instance at all — a clean constraint
//!      failure the host can catch, or something worse?
//!
//! Usage: cargo rustc --example agent_render --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::eval::{HostEffect, Rv};
use prism::eval_session::EvalSession;
use prism::{default_roots, host_ops_in, module_interface, show_effects, with_prelude, Sym};
use std::collections::HashSet;
use std::path::Path;

struct NullHost;

impl HostEffect for NullHost {
    fn call(&mut self, op: Sym, _args: &[Rv]) -> Result<Rv, String> {
        Err(format!("no host op `{op}`"))
    }
}

/// A miniature of the real mirror: the value vocabulary, the intent, a type with
/// a render, and a type without one.
const SRC: &str = r#"
pub type UITree = Text(String) | Badge(String) | Row(List(UITree)) | Card { title: Option(String), children: List(UITree) }

pub type Slot = Page | Inline | CardSlot

pub type Intent = Intent { slot: Slot, purpose: Option(String) }

pub type Person = Person { name: String, age: Int }

pub class Render(a)
  render : (a, Intent) -> Option(UITree)

instance renderPerson : Render(Person)
  fn render(p, i) = Some(Card { title = Some(p.name), children = [Text(p.name)] })

pub type Plain = Plain { n: Int }
"#;

fn main() {
    let full = with_prelude(SRC);
    let roots = default_roots(Path::new("."));

    println!("=== 1. interface entries mentioning class / instance ===");
    match module_interface(SRC, &full, &roots) {
        Ok(iface) => {
            for e in &iface.entries {
                if e.kind.contains("class") || e.kind.contains("instance") {
                    println!("[{}] name={} sig={}", e.kind, e.name, e.signature);
                }
            }
        }
        Err(e) => println!("interface ERR: {e}"),
    }

    let session = EvalSession::build(&full, &roots).expect("build session");
    let ops: HashSet<Sym> = host_ops_in(SRC);
    let mut host = NullHost;

    let value = r#"Person { name = "Ada", age = 36 }"#;
    let intent = r#"Intent { slot = CardSlot, purpose = None }"#;

    println!();
    println!("=== 2. resolution: bare vs explicit `using` ===");
    let attempts = [
        ("auto", format!("render({value}, {intent})")),
        (
            "using-tailing",
            format!("render({value}, {intent}) using renderPerson"),
        ),
        (
            "using-arg",
            format!("render({value}, {intent}, using renderPerson)"),
        ),
    ];
    let mut auto_worked = None;
    for (label, expr) in &attempts {
        match session.eval(expr, &ops, &mut host) {
            Ok((v, ty, eff)) => {
                println!("[{label}] OK   = {} : {} ! {}", v.show(), ty.show(), show_effects(&eff));
                if auto_worked.is_none() {
                    auto_worked = Some(expr.clone());
                }
            }
            Err(e) => {
                let first = e.to_string().lines().next().unwrap_or("").to_string();
                println!("[{label}] ERR  {first}");
            }
        }
    }

    println!();
    println!("=== 3. the returned value as an `Rv` ===");
    if let Some(expr) = &auto_worked {
        match session.eval(expr, &ops, &mut host) {
            Ok((v, _, _)) => println!("{v:?}"),
            Err(e) => println!("ERR: {e}"),
        }
    } else {
        println!("(nothing resolved)");
    }

    println!();
    println!("=== 4. a type with no instance ===");
    match session.eval(&format!("render(Plain {{ n = 1 }}, {intent})"), &ops, &mut host) {
        Ok((v, ty, _)) => println!("UNEXPECTEDLY OK: {} : {}", v.show(), ty.show()),
        Err(e) => println!("ERR (the good case): {e}"),
    }

    println!();
    println!("=== 5. deferring: an instance that returns None ===");
    let defer_src = r#"
pub type Quiet = Quiet { n: Int }
instance renderQuiet : Render(Quiet)
  fn render(q, i) = None
"#;
    let mut deferred = String::from(SRC);
    deferred.push_str(defer_src);
    if let Ok(s2) = EvalSession::build(&with_prelude(&deferred), &roots) {
        let ops2 = host_ops_in(&deferred);
        match s2.eval(
            &format!("render(Quiet {{ n = 7 }}, {intent})"),
            &ops2,
            &mut host,
        ) {
            Ok((v, ty, _)) => println!("OK: {} : {}", v.show(), ty.show()),
            Err(e) => println!("ERR: {e}"),
        }
    } else {
        println!("session build failed for the deferring case");
    }
}
