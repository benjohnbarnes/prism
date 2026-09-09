//! Spike: the effect-trap bridge (ported from prism-fork).
//!
//! A program declares its host capability as an effect (`effect Http { get_url … }`);
//! `main` performs `get_url(…)` with no handler, so the operation unwinds to the
//! top of the stack and — because it is in the declared host surface — traps to a
//! host `HostEffect` impl, which services it and the run resumes with the result.

use std::collections::HashSet;
use std::io::{BufReader, Cursor};

use prism::eval::{HostEffect, Rv};
use prism::{host_ops_in, interpret_with_host, type_schemas_of, with_prelude, Sym};

/// A trivial host: answers `get_url(url)` with a canned body; rejects anything else.
struct WebHost {
    body: String,
}

impl HostEffect for WebHost {
    fn call(&mut self, op: Sym, args: &[Rv]) -> Result<Rv, String> {
        match op.to_string().as_str() {
            "get_url" => {
                let _url = match args.first() {
                    Some(Rv::Str(s)) => s.clone(),
                    other => return Err(format!("get_url: bad arg {other:?}")),
                };
                Ok(Rv::Str(self.body.clone()))
            }
            other => Err(format!("unrecognized host op `{other}`")),
        }
    }
}

fn main() {
    // User source (no prelude): the effect *declares* the host capability.
    let user_src = "effect Http\n  get_url(String) : String\nfn main() = get_url(\"https://example.com\")";

    // (a) reflection: the declared host-effect surface.
    let surface: HashSet<Sym> = host_ops_in(user_src);
    println!("host surface: {:?}", surface);

    // (b) reflection: structural type schemas (no deriving needed for the schema).
    let types_src = "type Book = Book { title : String, pages : Int }\n";
    println!("book schema: {}", type_schemas_of(types_src).unwrap());

    // (c) the effect trap: run `main`, service `get_url` through the host.
    let full = with_prelude(user_src);
    let mut out: Vec<u8> = Vec::new();
    let mut input = BufReader::new(Cursor::new(Vec::new()));
    let mut host = WebHost {
        body: "<html>ok</html>".to_string(),
    };
    match interpret_with_host(&full, surface, &mut out, &mut input, &mut host) {
        Ok(run) => {
            println!("run.value = {}", run.value.show());
            println!("run.term  = {}", run.term.trim());
        }
        Err(e) => println!("RUN ERR: {e}"),
    }
}
