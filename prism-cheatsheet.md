# Prism — working cheat sheet

**Keep this short: non-obvious facts only.** No history, no rationale, no "we
measured". If it can be read off `prism/lib/**` or the spec at a glance, leave it
out. Where this sheet and the tree disagree, the tree wins.

Spec `prism/docs/src/spec.md` · corpus `prism/examples/*.pr` · prelude `prism/lib/prelude.pr` ·
CLI: `prism check FILE`, `prism run FILE` (no LLVM), `prism dump types FILE` — inferred types **and effect rows**.

## Shape

Strict, impure, ML-family. **Effects live in the type** as a row: `fn f() : Int ! {Console}`
may read the console and nothing else, and `{}` is provably pure.

```text
fn add(a : Int, b : Int) : Int = a + b   -- `=`, not `::`. lowercase vars, uppercase ctors
"hello {name}"                           -- strings interpolate; \{ \} for literal braces
type Tally = Tally { evens: Int }        -- record: Tally { evens = 0 }, fields via t.evens
type Shape = Dot | Line(Int, Int)        -- sum; match is exhaustive
```

## Effects

```text
effect State
  get() : Int
  put(Int) : Unit

fn tick() : Int ! {State} =              -- performing an op is just calling it
  let n = get()
  put(n + 1)
  n

handle tick() with                       -- handling removes the label from the type
  get() resume k => \(s) -> k(s)(s)
  put(s2) resume k => \(_s) -> k(())(s2)
  return r => \(_s) -> r
```

- Resumption grades: `once` / `many` / `never`. `never` has no continuation.
- Rows **union** across calls: `{A} + {B} = {A, B}`. A handled op **vanishes** from the
  row, so effectful code reads as pure at its boundary.
- `error E(String)` declares an error — an effect label of its own. `throw E(x)` performs
  it; `try f(x) catch E(k) => ...` discharges it. Uncaught is a **compile** error naming
  what remains.
- **An unhandled *effect* is a runtime fault, not a compile error.** `check` gives `main`
  `! {IO, Secret}` happily and running it faults. `run_io` wraps `main` at desugar when it
  reaches a capability wrapper, so `main`'s row collapses to `{IO}`.

### Capabilities

The row names *which* part of the world: `Console`, `FileSystem`, `Random`, `Entropy`,
`Env`, `Clock`, `Output`. Non-obvious:

- `Random` is **seeded and replayable**; `Entropy` is fresh OS entropy. Key material wants
  `Entropy`.
- File **writes** and `System` (shell-out) are opaque `IO`, *not* capability operations —
  not interceptable or replayable the way the rest are.
- `Net` and `Preempt` are reserved but **unshipped**: no networking.
- Interpreting always grants everything, via `run_io`. Hard denial is the **wasm** build,
  where those builtins are simply absent.

### Effect polymorphism

Two unrelated things share the name:

```text
fn twice(f : (Int) -> Int ! {| e}, x : Int) = f(x) + f(x)   -- {| e}: an open row TAIL
effect Emit(a)                                              -- `a`: a parameter ON the effect
  emit(a) : Unit
```

- **An operation's own variables are bound, and instantiated per call site.** This is how
  one capability returns whatever type each caller asked for:

  ```text
  effect Read
    read(ReadKey(a)) : a

  fn both() : (Int, String) ! {Read} =       -- one row, two result types
    ( read(ReadKey { id = "clock" }), read(ReadKey { id = "url" }) )
  ```

  An unconstrained use generalises: with `effect Read { now() : a }`, an unannotated
  `fn tick() = now()` gives `forall a. () -> a ! {Read}`.
- **An effect's parameter is fixed once per row.** `effect Tag(a)` cannot hold both
  `Tag(Int)` and `Tag(String)` — `effect instantiation mismatch`, even reached through two
  helpers. Carry a result type on an **operation variable**, not on the effect's parameter.
- **A class takes exactly one type parameter.** `given` adds constraints, not parameters.
  The parameter may be higher-kinded (`Functor(f)`).
- A **phantom** type parameter is allowed (`type ReadKey(a) = ReadKey { id : String }`) and
  the *caller* chooses it, so nothing ties a key to a result type.

## Traps

- **No string-literal patterns** — `JObj(Cons(("n", JInt(n)), Nil))` is a parse error.
  Bind the fields and look the key up: `json_field(kvs, "n") ! {Fail}`. Integer, bool and
  constructor patterns are fine.
- **`let` takes no type annotation**: `let x : T = ...` is a parse error. Pin the type in a
  helper `fn`'s return type — which is also the cure for `from_json`'s ambiguity
  (`cannot infer the type for constraint Json.FromJson(_)`): annotate the *function*, not
  the call site.
- **`decode` is strict**: JSON at byte 0 and exact number lexemes, so `1.99` gives
  `number not exactly representable`. For foreign input, `decode_lossy(trim(body))`.
- **No block comments, no user-defined operators**; the offside rule delimits blocks.
- `var` mutation is real, but **local state cannot escape** — such a function stays pure
  (`Int -> Int`, empty row).
- Record update compiles to in-place mutation **only when uniquely owned** (reference
  counting + FBIP): write value semantics, get pointer writes.

## Host integration (Rust)

```rust
let src = prism::with_prelude("fn main() = print(1 + 2)");
let checked = prism::check(&src)?;                     // declarations, with effect rows
let run = prism::interpret(&src)?;                     // tree-walking interpreter, run.term
prism::interpret_io_on(src, roots, out, input, cfg);   // explicit stdio — the data channel
```

- Data crosses as plain strings/JSON over the interpreter's stdio. **"wire"**
  (`prism-core-hash-v2`, `lib/std/Wire.pr`) is a *binary envelope* for content addressing
  and freezing continuations — not a data channel.
- `prism patch serve` (JSON-over-stdio) adds or replaces code in a running session:
  `fetch`, `impact`, `create`, `submit`/`apply`, `behavior`, `commit`, `discard`, each
  pinned by the content-addressed Core digest.
- **There is no interpreter-only native build.** The `native` feature pulls LLVM *and*
  compiles the C runtime, and the interpreter FFIs a vendored musl libm that only that
  build compiles — so compile the libm alone (`tools/build-libm.sh`) and link it.
  `PRISM_STORE_PATH` relocates the store (default `~/.cache/prism/store`, which goes stale
  across versions).
