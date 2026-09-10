//! Spike: `EvalSession` — evaluate expressions "on the fly" against a session
//! built once (the REPL's primitive, exposed for a host). No `main` synthesis,
//! no stdout capture: each eval returns `(value_repr, type, effects)`.

use std::collections::HashSet;
use std::path::Path;

use prism::eval::{HostEffect, Rv};
use prism::eval_session::EvalSession;
use prism::{default_roots, host_ops_in, show_effects, with_prelude, Sym};

struct WebHost {
    body: String,
}

impl HostEffect for WebHost {
    fn call(&mut self, op: Sym, _args: &[Rv]) -> Result<Rv, String> {
        match op.to_string().as_str() {
            "get_url" => Ok(Rv::Str(self.body.clone())),
            other => Err(format!("unknown host op `{other}`")),
        }
    }
}

fn main() {
    let user_src = "effect Http\n  get_url(String) : String\n\n\
                    fn f(x : Int) : Int = x + 1\n\
                    fn g(x : Int) : Int = x * 10\n\
                    fn fetch(url : String) : String = get_url(url)\n";

    let session = EvalSession::build(&with_prelude(user_src), &default_roots(Path::new(".")))
        .expect("build session");
    let ops: HashSet<Sym> = host_ops_in(user_src);
    let mut host = WebHost {
        body: "<html>ok</html>".to_string(),
    };

    // (a) a pure compound expression.
    let (v, ty, eff) = session.eval("f(1) + g(2)", &ops, &mut host).expect("eval pure");
    println!("f(1) + g(2)     = {} : {} ! {}", v.show(), ty.show(), show_effects(&eff));

    // (b) a `let`-bound expression.
    let (v, ty, eff) = session.eval("let x = f(5) in g(x)", &ops, &mut host).expect("eval let");
    println!("let x=f(5) in g = {} : {} ! {}", v.show(), ty.show(), show_effects(&eff));

    // (c) an effectful expression — `get_url` traps to the host and resumes.
    let (v, ty, eff) = session
        .eval("fetch(\"https://x\")", &ops, &mut host)
        .expect("eval effectful");
    println!("fetch(\"...\")    = {} : {} ! {}", v.show(), ty.show(), show_effects(&eff));
}
