//! Probe: value wire (`Serialize`) + `Stable` + `stable`-block schema evolution.
//!
//! Usage: cargo rustc --example agent_wire --no-default-features -- -C link-arg=.../libprism_libm.a

use prism::{check_on, default_roots, interpret_io_on, with_prelude, Config};
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

fn roots() -> Vec<prism::Root> {
    default_roots(Path::new("."))
}

fn cfg() -> Config {
    let mut c = Config::from_env();
    let dir = std::env::temp_dir().join(format!("agent-wire-store-{}", std::process::id()));
    c.update_flags(|f| f.store_path = Some(dir));
    c
}

fn check_case(label: &str, src: &str) {
    match check_on(&with_prelude(src), &roots()) {
        Ok(_) => println!("[{label}] CHECK OK"),
        Err(e) => println!("[{label}] CHECK ERR:\n{e}"),
    }
}

fn run_case(label: &str, src: &str) {
    let full = with_prelude(src);
    let mut input = BufReader::new(Cursor::new(Vec::new()));
    let mut output: Vec<u8> = Vec::new();
    match interpret_io_on(&full, &roots(), &mut output, &mut input, &cfg()) {
        Ok(_) => println!("[{label}] RUN:\n{}", String::from_utf8_lossy(&output)),
        Err(e) => println!("[{label}] RUN ERR:\n{e}"),
    }
}

fn main() {
    // A. Serialize round-trip (compact positional codec).
    run_case(
        "serialize-roundtrip",
        r#"import Wire (..)
type Point = Point { x : Int, y : Int } deriving (Show, Serialize)
fn roundtrip(p : Point) : Point =
  let (back, _rest) = decode(encode(p))
  back
fn main() =
  let p = Point { x = 1, y = 2 }
  println("len: {wire_len(encode(p))}")
  println("roundtrip: {show(roundtrip(p))}")
"#,
    );

    // B. Unannotated decode: the concrete type must be pinned -> check error.
    check_case(
        "unannotated-decode",
        r#"import Wire (..)
type Point = Point { x : Int, y : Int } deriving (Serialize)
fn bad() =
  let (x, _rest) = decode(encode(Point { x = 1, y = 2 }))
  x
"#,
    );

    // C. Stable seal round-trip.
    run_case(
        "stable-roundtrip",
        r#"import Wire (..)
type Meters = Meters(Int) deriving (Eq, Show, Serialize, Stable)
fn decode_meters(bs : Bytes) : Meters = wire_decode_stable(bs)
fn main() =
  println(show(decode_meters(wire_encode_stable(Meters(5)))))
"#,
    );

    // D. stable block: rungs + migrations + decode_ladder (V1 frame -> current).
    run_case(
        "stable-ladder",
        r#"import Wire (..)
type Calling = Sorcerer | Wizard deriving (Eq, Show, Serialize, Stable)
stable Sheet {
  V1 = { hero: String, level: Int },
  V2 = { ..V1, spell_slots: Int = 2 },
  migrations {
    V1 -> V2 = auto
  }
}
fn main() =
  let current = Sheet { hero = "Mira", level = 7, spell_slots = 5 }
  let (v1, loss) = Sheet.V1.downgrade(current)
  let restored = Sheet.V1.upgrade(v1)
  println("downgraded hero: {v1.hero}")
  println("dropped: {loss_names(loss)}")
  println("restored slots: {restored.spell_slots}")
  let v1_frame = wire_encode_stable(v1)
  let from_v1 = decode_ladder_Sheet(v1_frame)
  println("V1 frame -> slots: {from_v1.spell_slots}")
"#,
    );

    // E. Editing a frozen rung in place must fail (digest moved).
    check_case(
        "frozen-rung-edit",
        r#"import Wire (..)
type Calling = Sorcerer | Wizard | Bard deriving (Eq, Show, Serialize, Stable)
stable PlayerManual {
  V1 = { hero: String, calling: Calling, level: Int, nickname: String = "" } frozen "83ec19c79b74ffde",
  V2 = { ..V1, signature_spell: String = "Magic Missile" } frozen "4629e1366adf6a75",
  V3 = { ..V2, spell_slots: Int = 2, inspiration: Int = 0 },
  migrations {
    V1 -> V2 = auto
    V2 -> V3 = auto
    V1 -> V3 = auto
  }
}
"#,
    );

    // F. Codegen render: the host knows only the type NAME; a generated one-liner
    //    `show(wire_decode_stable(bs) : T)` pretty-prints the value structurally.
    run_case(
        "codegen-render",
        r#"import Wire (..)
type Point = Point { x : Int, y : Int } deriving (Show, Serialize, Stable)
type Request = Request { query: String, limit: Int, at: Point } deriving (Show, Serialize, Stable)
fn decode_request(bs : Bytes) : Request = wire_decode_stable(bs)
fn render(bs : Bytes) : String = show(decode_request(bs))
fn main() =
  let r = Request { query = "hello", limit = 3, at = Point { x = 1, y = 2 } }
  println(render(wire_encode_stable(r)))
"#,
    );

    // G. Bridge: a Stable/Serialize value decoded from wire, then converted to the
    //    dynamic Json tree and encoded — so the host can hold compact wire and
    //    materialize JSON on demand (and vice versa via FromJson).
    run_case(
        "wire-to-json-bridge",
        r#"import Wire (..)
import Json (..)
type Point = Point { x : Int, y : Int } deriving (Show, Serialize, Stable, ToJson, FromJson)
fn decode_point(bs : Bytes) : Point = wire_decode_stable(bs)
fn decode_point_from_json(j : Json) : Point = from_json(j)
fn wire_to_json(bs : Bytes) : String = encode(to_json(decode_point(bs)))
fn json_to_wire(s : String) : Bytes =
  match decode(s) of
    Ok(j) => wire_encode_stable(decode_point_from_json(j))
    Err(_e) => fail()
fn main() =
  let p = Point { x = 1, y = 2 }
  println("wire -> json: {wire_to_json(wire_encode_stable(p))}")
  let round = decode_point(json_to_wire(wire_to_json(wire_encode_stable(p))))
  println("json -> wire -> value: {show(round)}")
"#,
    );
}
