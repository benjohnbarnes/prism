//! An effect operation's type variables are the operation's own.
//!
//! A signature that mentions one — `ask(a) : Int` — has to build a *closed* scheme.
//! A variable left free joins the environment's free variables, which generalization
//! anchors on, so any declaration that spells a signature variable the same way is
//! generalized without it, and its own correct `given` constraint is refused as
//! ambiguous.

use prism::{check_on, report};

/// An ordinary class, a declaration that mentions its own type variable under a
/// constraint, and an operation polymorphic in a variable it names in a parameter.
const SHARING_A_NAME: &str = "class C(a)\n  c : (a) -> Int\n\n\
effect Ask\n  ask(a) : Int\n\n\
fn needs(x : a) : Int given C(a) = c(x)\n\n\
fn main() = 1\n";

/// The same program with the operation's variable spelled differently, so nothing
/// shares a name — the control for what the fault is.
const NOT_SHARING: &str = "class C(a)\n  c : (a) -> Int\n\n\
effect Ask\n  ask(zz) : Int\n\n\
fn needs(x : a) : Int given C(a) = c(x)\n\n\
fn main() = 1\n";

/// The operation alone: no other declaration mentions `a` under a constraint.
const OPERATION_ALONE: &str = "effect Ask\n  ask(a) : Int\n\nfn main() = 1\n";

/// The declaration alone: no operation mentions `a`.
const DECLARATION_ALONE: &str = "class C(a)\n  c : (a) -> Int\n\n\
fn needs(x : a) : Int given C(a) = c(x)\n\n\
fn main() = 1\n";

/// What the operation is for: one polymorphic op, performed at two types.
const PERFORMED_AT_TWO_TYPES: &str = "effect Ask\n  ask(a) : Int\n\n\
fn one() : Int = ask(1)\n\
fn two() : Int = ask(\"x\")\n";

/// A monomorphic operation, unaffected by any of this.
const MONOMORPHIC: &str = "effect Ask\n  ask(Int) : Int\n\nfn one() : Int = ask(1)\n";

/// Checked with nothing resolved for it: `Int` is built in, and every name these
/// programs use is declared in the program itself.
fn check(src: &str) -> Result<(), String> {
    check_on(src, &[]).map(|_| ()).map_err(|e| e.to_string())
}

#[test]
fn an_operation_variable_is_not_shared_with_another_declaration() {
    if let Err(e) = check(SHARING_A_NAME) {
        panic!("the operation's variable reached a declaration beside it: {e}");
    }
    let pipeline = report(SHARING_A_NAME);
    assert!(
        !pipeline.contains("ambiguous constraint"),
        "the pipeline refuses it too:\n{pipeline}"
    );
}

#[test]
fn spelling_the_operation_variable_differently_changes_nothing_else() {
    if let Err(e) = check(NOT_SHARING) {
        panic!("nothing shares a name here: {e}");
    }
}

#[test]
fn neither_half_refuses_on_its_own() {
    if let Err(e) = check(OPERATION_ALONE) {
        panic!("the operation alone: {e}");
    }
    if let Err(e) = check(DECLARATION_ALONE) {
        panic!("the declaration alone: {e}");
    }
}

#[test]
fn the_operation_is_polymorphic_at_each_perform_site() {
    if let Err(e) = check(PERFORMED_AT_TWO_TYPES) {
        panic!("one op performed at two types: {e}");
    }
}

#[test]
fn a_monomorphic_operation_is_unaffected() {
    if let Err(e) = check(MONOMORPHIC) {
        panic!("monomorphic op: {e}");
    }
}
