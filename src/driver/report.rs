//! The human-facing `report` surface: run the whole pipeline over a source and
//! render every phase (tokens, ast, types, core, fbip, llvm, run) as one text
//! block, plus the shape-digest gate primitive. Split out of the driver so
//! `mod.rs` holds the compile/check entry points and this module holds their
//! rendered pipeline face. Every external path (`prism::report`,
//! `prism::report_on`, `prism::shape_digests_of`) resolves through the re-export
//! in `mod.rs`, so the split is invisible to callers.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::core::fbip::borrow_sigs;
use crate::core::{elaborate_typed, insert_rc, pp_core_pretty, reuse, Digest, ElaboratedCore};
use crate::error::Error;
use crate::eval::{run, Rv};
use crate::lex::lex;
use crate::parse::{parse, ParseResult};
use crate::resolve::{default_roots, resolve_modules_in, Root};
use crate::syntax::ast::{DataDecl, Ty};
use crate::syntax::desugar::desugar;
use crate::types::{check as typecheck, Checked};

#[cfg(feature = "native")]
use crate::codegen::emit_llvm_with_native_kont_table;
#[cfg(feature = "native")]
use crate::core::{fip_annots, hash_program};

#[cfg(feature = "native")]
use super::identity::{native_kont_table_of, NativeKontIdentityRows};
use super::query::section;
#[cfg(feature = "native")]
use super::query::strip_target;
use super::verify::{fip_check, replayable_check};
#[cfg(feature = "native")]
use super::{finish_lowered, hash_meta, lower_opt};
use super::{frontend, Config};

pub(super) fn types_section(checked: &Checked) -> String {
    let mut s = String::new();
    for d in &checked.decls {
        writeln!(s, "{} : {}", d.name, d.ty.show()).unwrap();
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

    let program = match resolve_modules_in(program, roots) {
        Ok(p) => p,
        Err(e) => {
            section(&mut out, "resolve", &render(e));
            return out;
        }
    };

    let program = match desugar(program) {
        Ok(p) => p,
        Err(e) => {
            section(&mut out, "types", &render(e.into()));
            return out;
        }
    };
    let checked = match typecheck(&program) {
        Ok(c) => c,
        Err(e) => {
            section(&mut out, "types", &render(e.into()));
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
    let core = ElaboratedCore(core);
    section(&mut out, "core (cbpv)", pp_core_pretty(&core).trim_end());

    if let Err(e) = fip_check(&program, &checked, &core) {
        section(&mut out, "fip", &render(e));
        return out;
    }

    if let Err(e) = replayable_check(&program, &checked) {
        section(&mut out, "replayable", &render(e));
        return out;
    }

    let sigs = borrow_sigs(&program);
    section(
        &mut out,
        "fbip (rc)",
        pp_core_pretty(&reuse(&insert_rc(&core, &sigs))).trim_end(),
    );

    #[cfg(feature = "native")]
    match lower_opt(
        typed,
        &verify_env,
        &checked.ctors,
        &checked.op_grades(),
        cfg,
    )
    .and_then(|lowered| {
        let ctors = lowered.ctors.clone();
        finish_lowered(lowered, &sigs).map(|core| (core, ctors))
    }) {
        Ok((lowered, ctors)) => {
            let hashes = hash_program(&core, &hash_meta(&checked, &sigs, &fip_annots(&program)));
            match native_kont_table_of(&hashes, roots, cfg, NativeKontIdentityRows::Portable)
                .and_then(|native_kont_table| {
                    emit_llvm_with_native_kont_table(
                        &lowered,
                        &ctors,
                        &native_kont_table,
                        cfg.flags.native_kont_frames,
                    )
                    .map_err(Error::CodegenBackend)
                }) {
                Ok(ir) => section(&mut out, "llvm", strip_target(&ir).trim_end()),
                Err(e) => section(&mut out, "llvm", &format!("(skipped: {e})")),
            }
        }
        Err(e) => section(&mut out, "llvm", &format!("(skipped: {e})")),
    }

    match run(&core) {
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

/// The structural schema of every datatype `src` defines, keyed by type name:
/// its type parameters, whether it is a newtype, and each constructor's fields
/// (named, for records) or positional argument types. Types nest by name
/// (`List(CatalogEntry)`, `Option(DefHash)`).
///
/// This is the enabler for host-side self-description (see
/// `docs/SELF-DESCRIPTION-DESIGN.md`): the host validates its own Loro documents
/// against the declared shape and derives default renderings from it. Companion to
/// `shape_digests_of`, which hashes the same shapes; this exposes them structurally.
///
/// # Errors
/// Fails if `src` does not parse, resolve, or type-check.
pub fn type_schemas_of(src: &str) -> Result<serde_json::Value, Error> {
    let (program, _, _) = frontend(src, &default_roots(Path::new(".")), &Config::from_env())?;
    let mut out = serde_json::Map::new();
    for d in &program.types {
        out.insert(d.name.clone(), data_schema_json(d));
    }
    Ok(serde_json::Value::Object(out))
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
