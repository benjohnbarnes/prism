//! The human-facing `report` surface: run the whole pipeline over a source and
//! render every phase (tokens, ast, types, core, fbip, llvm, run) as one text
//! block, plus the shape-digest gate primitive. Split out of the driver so
//! `mod.rs` holds the compile/check entry points and this module holds their
//! rendered pipeline face. Every external path (`prism::report`,
//! `prism::report_on`, `prism::shape_digests_of`) resolves through the re-export
//! in `mod.rs`, so the split is invisible to callers.

use std::{collections::BTreeMap, fmt::Write as _, path::Path};

use crate::core::{elaborate_typed, insert_rc, pp_core_pretty, reuse, Digest};
use crate::error::Error;
use crate::eval::{run, Rv};
use crate::lex::lex;
use crate::parse::{parse, ParseResult};
use crate::resolve::{default_roots, Root};
use crate::types::Checked;

#[cfg(feature = "native")]
use crate::core::fbip::borrow_sigs;
#[cfg(feature = "native")]
use crate::core::typed::effect_lower::FinishedLowering;
#[cfg(feature = "native")]
use crate::core::{fip_annots, hash_program};
#[cfg(feature = "native")]
use prism_native::emit_llvm_with_native_kont_table;

use super::front::{run_front, FrontRequest};
#[cfg(feature = "native")]
use super::identity::{native_kont_table_of, NativeKontIdentityRows};
use super::query::section;
#[cfg(feature = "native")]
use super::query::strip_target;
use super::verify::{fip_check, replayable_check};
#[cfg(feature = "native")]
use super::{finish_lowered, hash_meta, lower_opt};
// The rendered `fbip (rc)` section is not native-gated, so its borrow masks are
// needed on every target.
use super::{frontend, rc_borrow_sigs, with_prelude, Config};
use crate::syntax::ast::{DataDecl, Ty};

pub(super) fn types_section(checked: &Checked) -> String {
    let mut s = String::new();
    for d in &checked.defs.decls {
        writeln!(s, "{} : {}", d.name, checked.show_sig(d)).unwrap();
    }
    s
}

#[must_use]
pub fn report(src: &str) -> String {
    report_at(src, Path::new("."))
}

#[must_use]
pub fn report_at(src: &str, base: &Path) -> String {
    report_on(src, &default_roots(base), &Config::from_env())
}

/// Like [`report_at`], but against an explicit module search path.
// `cfg` drives the native-only Core/codegen phases; on wasm those are compiled
// out, so it is unused there.
#[cfg_attr(not(feature = "native"), allow(unused_variables))]
#[must_use]
pub fn report_on(src: &str, roots: &[Root], cfg: &Config) -> String {
    // Render a phase failure with the same span-aware ariadne report the CLI
    // shows for `run`/`build`/`check`, so `report` does not degrade to a bare
    // message.
    let render = |e: Error| e.render_plain(src, "<source>");
    let mut out = String::new();
    let tokens = match lex(src) {
        Ok((t, _)) => t,
        Err(e) => return render(e.into()),
    };
    let toks: Vec<String> = tokens.iter().map(|(_, t, _)| format!("{t:?}")).collect();
    section(&mut out, "tokens", &toks.join(" "));

    let ParseResult { program, .. } = match parse(src) {
        Ok(r) => r,
        Err(e) => {
            section(&mut out, "parse", &render(e.into()));
            return out;
        }
    };
    section(&mut out, "ast", &format!("{program:#?}"));

    // Route resolve, test stripping, the logical-contract check, desugar, and
    // typecheck through the one canonical front runner rather than a parallel
    // hand-rolled prefix, so this rendered pipeline cannot drift from a real
    // check. Elaboration and the fip / replayable validators continue below,
    // keeping their own per-phase sections (a checked program whose elaboration
    // later fails still shows its `types` section before the `core (cbpv)` error).
    let (program, checked) = match run_front(src, roots, cfg, FrontRequest::Report) {
        Ok(front) => front.into_program_checked(),
        Err(e) => {
            section(&mut out, "types", &render(e));
            return out;
        }
    };
    section(&mut out, "types", types_section(&checked).trim_end());

    let elaboration = match elaborate_typed(&program, &checked) {
        Ok(elaboration) => elaboration,
        Err(e) => {
            section(&mut out, "core (cbpv)", &render(e));
            return out;
        }
    };
    let (core, typed, verify_env) = elaboration.into_parts();
    section(&mut out, "core (cbpv)", pp_core_pretty(&core).trim_end());

    if let Err(e) = fip_check(&program, &checked, &core, &typed) {
        section(&mut out, "fip", &render(e));
        return out;
    }

    if let Err(e) = replayable_check(&program, &checked) {
        section(&mut out, "replayable", &render(e));
        return out;
    }

    let sigs = rc_borrow_sigs(&program, &checked, &core, cfg);
    section(
        &mut out,
        "fbip (rc)",
        pp_core_pretty(&reuse(&insert_rc(&core, &sigs))).trim_end(),
    );

    #[cfg(feature = "native")]
    match lower_opt(
        typed,
        &verify_env,
        &checked.defs.ctors,
        &checked.op_grades(),
        cfg,
    )
    .and_then(|lowered| {
        finish_lowered(lowered, &sigs, cfg).map(FinishedLowering::into_core_and_constructors)
    }) {
        Ok((lowered, ctors)) => {
            // Identity reads the declared masks; the augmented `sigs` above are
            // a reference-count decision and never part of the program's hash.
            let hashes = hash_program(
                &core,
                &hash_meta(&checked, &borrow_sigs(&program), &fip_annots(&program)),
            );
            match native_kont_table_of(&hashes, roots, cfg, NativeKontIdentityRows::Portable)
                .and_then(|native_kont_table| {
                    emit_llvm_with_native_kont_table(
                        &lowered,
                        &ctors,
                        &native_kont_table,
                        cfg.flags().native_kont_frames,
                    )
                    .map_err(Error::CodegenBackend)
                }) {
                Ok(ir) => section(&mut out, "llvm", strip_target(&ir).trim_end()),
                Err(e) => section(&mut out, "llvm", &format!("(skipped: {e})")),
            }
        }
        Err(e) => section(&mut out, "llvm", &format!("(skipped: {e})")),
    }

    let identity = super::execution::identity_of(src, roots, cfg);
    match run(&core, Some(&identity)) {
        Ok(r) => {
            let outs: Vec<String> = r.out.iter().map(Rv::show).collect();
            section(
                &mut out,
                "run",
                &format!("output: [{}]\nresult: {}", outs.join(", "), r.value.show()),
            );
        }
        Err(e) => section(&mut out, "run", &format!("error: {e}")),
    }
    out
}

/// The structural shape digest of every datatype and effect a source defines
/// (prelude included), keyed by name, full-length.
///
/// This is the format-identity gate primitive: commit the digests a persisted
/// type produces and a later edit that changes the wire layout (a new
/// constructor, a reordered field, a changed component type) moves the digest and
/// fails the committed golden, while a cosmetic edit leaves it untouched. A caller
/// snapshots or asserts on the entries for the types it persists.
///
/// # Errors
/// Fails if `src` does not parse, resolve, or type-check.
pub fn shape_digests_of(src: &str) -> Result<BTreeMap<String, Digest>, Error> {
    let (program, _, _) = frontend(src, &default_roots(Path::new(".")), &Config::from_env())?;
    Ok(crate::core::shape_digests(&program.types, &program.effects))
}

/// Extract each top-level declaration's doc comment (`-- |` Haddock runs) from
/// `src`, keyed by the declaration name. A `-- |` run continues over following
/// plain `--` lines and attaches to the next top-level declaration; a blank line
/// ends the run. This is the host's description channel: the LLM reads a
/// function's prose without a codegen round-trip.
#[must_use]
pub fn docs_of(src: &str) -> BTreeMap<String, String> {
    let mut docs = BTreeMap::new();
    let mut pending: Vec<String> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        if let Some(text) = trimmed.strip_prefix("-- |") {
            pending.push(text.trim_start().to_string());
            continue;
        }
        if trimmed.starts_with("--") {
            // A plain `--` line continues the doc run in progress (Haddock style).
            if !pending.is_empty() {
                let text = trimmed.strip_prefix("--").unwrap_or("").trim_start();
                pending.push(text.to_string());
            }
            continue;
        }
        if trimmed.is_empty() {
            pending.clear();
            continue;
        }
        if let Some(name) = decl_name(trimmed) {
            if !pending.is_empty() {
                docs.entry(name.to_string())
                    .or_insert_with(|| pending.join("\n"));
                pending.clear();
            }
        }
    }
    docs
}

/// The declared name at the head of a top-level declaration line, if any.
fn decl_name(line: &str) -> Option<&str> {
    let body = line.strip_prefix("pub ").unwrap_or(line);
    for kw in [
        "fn ", "type ", "effect ", "class ", "instance ", "op ", "ctr ", "law ",
    ] {
        if let Some(rest) = body.strip_prefix(kw) {
            let name = rest
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\''))
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// The structural schema of every datatype `src` defines, keyed by type name: its
/// type parameters, whether it is a newtype, and each constructor's fields (named,
/// for records) or positional argument types. Types nest by name
/// (`List(CatalogEntry)`, `Option(DefHash)`).
///
/// This is the host-side self-description primitive: the host validates its own
/// documents against the declared shape and derives default renderings from it.
/// Companion to [`shape_digests_of`], which hashes the same shapes; this exposes
/// them structurally (for the LLM's benefit and for schema validation).
///
/// # Errors
/// Fails if `src` does not parse, resolve, or type-check.
pub fn type_schemas_of(src: &str) -> Result<serde_json::Value, Error> {
    type_schemas_of_in(src, &default_roots(Path::new(".")), &Config::from_env())
}

/// [`type_schemas_of`] with explicit resolution roots and configuration (the
/// variant a host uses when a module imports other host modules).
///
/// The prelude is prepended so `deriving (Show, Eq, …)` clauses resolve, and the
/// prelude's own types are elided from the result (only the user's types are
/// returned).
///
/// # Errors
/// Fails if `src` does not parse, resolve, or type-check.
pub fn type_schemas_of_in(
    src: &str,
    roots: &[Root],
    cfg: &Config,
) -> Result<serde_json::Value, Error> {
    let prelude: std::collections::BTreeSet<String> = match schema_map("", roots, cfg) {
        Ok(m) => m.keys().cloned().collect(),
        Err(_) => std::collections::BTreeSet::new(),
    };
    let map = schema_map(src, roots, cfg)?;
    let user: serde_json::Map<String, serde_json::Value> = map
        .into_iter()
        .filter(|(k, _)| !prelude.contains(k))
        .collect();
    Ok(serde_json::Value::Object(user))
}

fn schema_map(
    src: &str,
    roots: &[Root],
    cfg: &Config,
) -> Result<serde_json::Map<String, serde_json::Value>, Error> {
    let (program, _, _) = frontend(&with_prelude(src), roots, cfg)?;
    let mut out = serde_json::Map::new();
    for d in &program.types {
        out.insert(d.name.clone(), data_schema_json(d));
    }
    Ok(out)
}

/// One datatype's schema as JSON: `{ params, newtype, constructors: [...] }`.
fn data_schema_json(d: &DataDecl) -> serde_json::Value {
    let ctors: Vec<serde_json::Value> = d
        .ctors
        .iter()
        .map(|c| match &c.fields {
            // Record constructor: named fields (`X { a : T }`).
            Some(fields) => serde_json::json!({
                "name": c.name,
                "fields": fields
                    .iter()
                    .map(|(n, t)| serde_json::json!({ "name": n, "type": ty_str(t) }))
                    .collect::<Vec<_>>(),
            }),
            // Positional constructor (`X(T, U)`).
            None => serde_json::json!({
                "name": c.name,
                "args": c.args.iter().map(ty_str).collect::<Vec<_>>(),
            }),
        })
        .collect();
    serde_json::json!({ "params": d.params, "newtype": d.newtype, "constructors": ctors })
}

/// Render a surface type as the string a reader would write (`List(CatalogEntry)`,
/// `Option(DefHash)`, `String`). Enough for the schema consumers (validation and the
/// default renderer); rarer forms fall back to their debug spelling.
fn ty_str(t: &Ty) -> String {
    match t {
        Ty::Int => "Int".to_string(),
        Ty::I64 => "I64".to_string(),
        Ty::U64 => "U64".to_string(),
        Ty::Bool => "Bool".to_string(),
        Ty::Unit => "Unit".to_string(),
        Ty::Float => "Float".to_string(),
        Ty::Char => "Char".to_string(),
        Ty::Str => "String".to_string(),
        Ty::Var(s) => s.clone(),
        Ty::Con(name, args) | Ty::App(name, args) => {
            if args.is_empty() {
                name.clone()
            } else {
                format!("{name}({})", args.iter().map(ty_str).collect::<Vec<_>>().join(", "))
            }
        }
        Ty::Tuple(ts) => format!("({})", ts.iter().map(ty_str).collect::<Vec<_>>().join(", ")),
        other => format!("{other:?}"),
    }
}
