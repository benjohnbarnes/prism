//! Spike probe: validate `interpret_io_on` — the streaming host<->tool data
//! channel (explicit stdin/stdout/argv), which is how a Rust host will pass
//! JSON into a generated tool and read JSON back.
//!
//! Usage: cargo rustc --example agent_io --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::{default_roots, interpret_io_on, with_prelude, Config};
use std::io::{BufReader, Cursor};
use std::path::Path;

fn main() {
    let src = with_prelude(
        "fn main() =\n  let line = read_line()\n  println(\"echo: {line}\")",
    );
    let roots = default_roots(Path::new("."));
    let cfg = Config::from_env();

    // Host supplies input over a byte buffer and captures streamed output.
    let mut input = BufReader::new(Cursor::new(b"hello world\n".to_vec()));
    let mut output: Vec<u8> = Vec::new();

    let run = interpret_io_on(&src, &roots, &mut output, &mut input, &cfg).unwrap();
    println!("[term]     {}", run.term.trim_end());
    println!("[streamed] {}", String::from_utf8_lossy(&output).trim_end());
    println!("[steps]    {}", run.steps);
}
