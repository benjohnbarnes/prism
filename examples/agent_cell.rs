//! Spike: reproduce the workbook's cell-recompute model on our host fork.
//!
//! The reference (workbook/src/lang/engine.ts) assembles each cell as a top-level
//! `fn cell_<id>() = body` definition and — to "invoke" one — synthesizes a thin
//! `fn main() = println(to_json_string(cell_<id>()))` and runs it. There is no
//! reflective `apply(name, value)` primitive; the named-entry seam is this
//! generated `main`. Effectful cells perform a declared host effect and run under
//! the effect-trap bridge (`interpret_with_host`).

use std::collections::HashSet;
use std::io::{BufReader, Cursor};
use std::path::Path;

use prism::eval::{HostEffect, Rv};
use prism::{
    default_roots, host_ops_in, interpret_io_on, interpret_with_host, with_prelude, Config, Sym,
};

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

/// Run `main` over `program` (already prelude'd) and return the captured print.
fn run_program(program: &str) -> String {
    let mut out: Vec<u8> = Vec::new();
    let mut input = BufReader::new(Cursor::new(Vec::new()));
    let cfg = Config::from_env();
    interpret_io_on(program, &default_roots(Path::new(".")), &mut out, &mut input, &cfg)
        .expect("run");
    String::from_utf8_lossy(&out).trim().to_string()
}

fn main() {
    // -- pure cells: top-level defs; invoke one via a thin main -----------------
    let cells = "import Json (..)\nfn cell_a() = 40\nfn cell_b() = 2\nfn cell_total() = cell_a() + cell_b()";

    let invoke = |name: &str| {
        run_program(&with_prelude(&format!(
            "{cells}\nfn main() = println(to_json_string({name}()))"
        )))
    };
    println!("cell_a()     = {}", invoke("cell_a"));
    println!("cell_b()     = {}", invoke("cell_b"));
    println!("cell_total() = {}", invoke("cell_total"));

    // -- effectful cell: perform a declared host effect via the trap ------------
    let user_src = "effect Http\n  get_url(String) : String\nfn cell_fetch() = get_url(\"https://x\")";
    let surface: HashSet<Sym> = host_ops_in(user_src);
    let full = with_prelude(&format!(
        "import Json (..)\n{user_src}\nfn main() = println(to_json_string(cell_fetch()))"
    ));
    let mut out: Vec<u8> = Vec::new();
    let mut input = BufReader::new(Cursor::new(Vec::new()));
    let mut host = WebHost {
        body: "<html>ok</html>".to_string(),
    };
    match interpret_with_host(&full, surface, &mut out, &mut input, &mut host) {
        Ok(_) => println!("cell_fetch() = {}", String::from_utf8_lossy(&out).trim()),
        Err(e) => println!("cell_fetch() ERR: {e}"),
    }
}
