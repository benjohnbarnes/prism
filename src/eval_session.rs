//! Evaluate an expression "on the fly" against a session built once from a
//! program — the primitive the REPL uses (`eval_chained`), exposed for a host.
//!
//! Build once (parse → resolve → desugar → check → elaborate → globals), then
//! `eval` any number of expressions, each returning `(value_repr, type, effects)`.
//! The effect-trap bridge is threaded through, so an expression performing an op
//! in the declared host surface is serviced by the host and the run resumes.

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;

use crate::core::{builtin_arities, elaborate, elaborate_expr_defs, CoreFn};
use crate::error::Error;
use crate::eval::{globals, HostEffect, Machine, Rv};
use crate::parse::{parse, parse_expr};
use crate::resolve::{import_bindings, resolve_expr, resolve_modules_in, Root};
use crate::sym::Sym;
use crate::syntax::ast::{Core, Expr, S};
use crate::syntax::desugar::{desugar, desugar_expr};
use crate::tc::{check, infer_checked_expr};
use crate::types::{Checked, Effects, Type};

/// A built program session: the artifacts needed to type-check and run a
/// standalone expression in the program's context. Built once, evaluated many
/// times — no per-call front-end, no `main` synthesis, no stdout capture.
pub struct EvalSession {
    checked: Checked,
    globals: BTreeMap<Sym, CoreFn>,
    arity: BTreeMap<String, usize>,
    consts: BTreeMap<String, S<Expr<Core>>>,
    imports: BTreeMap<String, String>,
}

impl std::fmt::Debug for EvalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalSession")
            .field("definitions", &self.globals.len())
            .field("imports", &self.imports.len())
            .finish_non_exhaustive()
    }
}

impl EvalSession {
    /// Build a session from a program (the prelude'd source of a module plus its
    /// imports, resolved over `roots`).
    ///
    /// # Errors
    /// Fails on any front-end error (parse/resolve/type-check).
    pub fn build(src: &str, roots: &[Root]) -> Result<Self, Error> {
        let program = parse(src)?.program;
        let imports = import_bindings(&program, roots)?;
        let program = resolve_modules_in(program, roots)?;
        let program = desugar(program)?;
        let checked = check(&program)?;
        let core = elaborate(&program, &checked)?;
        let mut arity: BTreeMap<String, usize> = checked
            .defs
            .decls
            .iter()
            .map(|d| (d.name.clone(), d.params.len()))
            .collect();
        builtin_arities(&mut arity);
        let consts = program
            .fns
            .iter()
            .filter(|d| d.konst)
            .map(|d| (d.name.clone(), d.body.clone()))
            .collect();
        Ok(Self {
            checked,
            globals: globals(&core),
            arity,
            consts,
            imports,
        })
    }

    /// Infer an expression's type and effect row **without** running it — the
    /// pre-eval gate: a disallowed effect is refused before it is performed.
    ///
    /// # Errors
    /// Fails on a parse/type error in the expression.
    pub fn infer(&self, expr: &str) -> Result<(Type, Effects), Error> {
        let mut surface = parse_expr(expr)?;
        resolve_expr(&mut surface, &self.imports)?;
        let e = desugar_expr(&surface)?;
        let inferred = infer_checked_expr(&self.checked, &e, false)?;
        Ok((inferred.ty, inferred.effects))
    }

    /// Evaluate one expression in this session, returning the raw value plus its
    /// inferred type and effect row. An op in `ops` that reaches the top with no
    /// handler is serviced by `host` (the effect-trap bridge).
    ///
    /// # Errors
    /// Fails on a parse/type error in the expression, a runtime fault, or a host
    /// error servicing an operation.
    pub fn eval(
        &self,
        expr: &str,
        ops: &HashSet<Sym>,
        host: &mut dyn HostEffect,
    ) -> Result<(Rv, Type, Effects), Error> {
        let mut surface = parse_expr(expr)?;
        resolve_expr(&mut surface, &self.imports)?;
        let e = desugar_expr(&surface)?;
        let inferred = infer_checked_expr(&self.checked, &e, false)?;
        let (comp, synthesized) = elaborate_expr_defs(
            &self.checked,
            &e,
            &self.arity,
            Some(&inferred.facts),
            &self.consts,
        )?;
        let mut globals = self.globals.clone();
        for f in synthesized {
            globals.insert(f.name, f);
        }
        // No IO at the eval boundary: the value is returned, not printed. A sink
        // keeps any stray `print`/`exit` from touching the host's real stdio.
        let mut sink: Vec<u8> = Vec::new();
        let mut input = Cursor::new(Vec::<u8>::new());
        let value = {
            let mut m = Machine::new(&globals, &mut sink, &mut input).with_host(ops.clone(), host);
            m.eval(&comp).map_err(Error::RuntimeEvaluation)?
        };
        Ok((value, inferred.ty, inferred.effects))
    }
}
