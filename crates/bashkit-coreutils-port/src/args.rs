//! Args mode — port a uutils utility's `uu_app()` clap definition into a
//! standalone Rust module bashkit can compile without uucore/Fluent.
//!
//! Algorithm:
//! 1. Read `<uutils>/src/uu/<util>/src/<util>.rs` and parse with syn.
//! 2. Read `<uutils>/src/uu/<util>/locales/en-US.ftl` (flat key=value).
//! 3. Walk the AST and rewrite uucore-specific calls in-place:
//!    - `translate!("k")`              -> `String::from("<value from ftl>")`
//!    - `uucore::crate_version!()`     -> `env!("CARGO_PKG_VERSION")`
//!    - `uucore::format_usage(x)`      -> `format_usage(x)` (local shim)
//!    - `uucore::localized_help_template("x")` -> the chained call is dropped
//!    - `uucore::clap_localization::configure_localized_command(cmd)` -> `cmd`
//!      (uutils wraps the Command in a localization-aware adapter that
//!      pulls in Fluent; bashkit doesn't need it, so we replace the
//!      call with its first argument)
//!    - `Arg::…env("FOO")…` — chain step elided AND harvested into a
//!      sidecar `<UTIL>_ENV_DEFAULTS` table consumed by the bashkit-side
//!      virtual-env shim (TM-INF-024).
//! 4. Emit a generated file containing only:
//!    - `mod options { pub static ... }` (verbatim from source)
//!    - `pub fn <util>_command() -> clap::Command` (rewritten `uu_app`)
//!    - `fn format_usage(s: &str) -> String` (vendored shim)
//!    - `pub static <UTIL>_ENV_DEFAULTS: &[clap_env::EnvDefault]` table.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use proc_macro2::TokenStream;
use quote::quote;
use syn::visit_mut::{self, VisitMut};
use syn::{
    Expr, ExprCall, ExprMacro, Item, ItemFn, ItemImpl, ItemMod, LitStr, Stmt, Type, parse_quote,
};

pub fn run(uutils_dir: &Path, util: &str, rev: &str) -> Result<String> {
    let src_path = uutils_dir
        .join("src/uu")
        .join(util)
        .join("src")
        .join(format!("{util}.rs"));
    let ftl_path = uutils_dir
        .join("src/uu")
        .join(util)
        .join("locales/en-US.ftl");

    let primary_src = std::fs::read_to_string(&src_path)
        .with_context(|| format!("read uutils source {}", src_path.display()))?;
    let ftl = std::fs::read_to_string(&ftl_path)
        .with_context(|| format!("read uutils ftl {}", ftl_path.display()))?;

    let translations = parse_ftl(&ftl);
    let primary_file = syn::parse_file(&primary_src).context("parse uutils source as rust")?;

    // `uu_app` usually lives in `<util>.rs`, but a few utilities (e.g.
    // tee) split it into a sibling `cli.rs` and re-export. Find the
    // file that actually defines `fn uu_app` and treat that as the
    // source of truth for the rest of the algorithm: option-key
    // constants, helper inlining, and the `mod options` lookup all
    // need to see whichever file owns the clap definition.
    let (src_path, file) = if find_fn(&primary_file, "uu_app").is_some() {
        (src_path.clone(), primary_file)
    } else if let Some((sib_path, sib_file)) = find_sibling_file_with_fn(&src_path, "uu_app")? {
        (sib_path, sib_file)
    } else {
        bail!(
            "could not find `fn uu_app` in {} or its sibling files",
            src_path.display()
        );
    };

    // Two option-key declaration styles in the wild:
    // 1. `mod options { pub static FOO: &str = "foo"; ... }` (cat, tac,
    //    truncate, stat, shuf). uu_app() refers to keys via
    //    `options::FOO`.
    // 2. Module-level `const OPT_FOO: &str = "foo";` / `static OPT_FOO`
    //    (mktemp, realpath, readlink, od). uu_app() refers to keys by
    //    bare name (`OPT_FOO`).
    // 3. `pub mod options { ... }` declared in a sibling file (ls/config.rs)
    //    with nested submodules (`options::format::COLUMNS`). The util's
    //    own .rs imports `use crate::options;` or `use config::options::*`.
    //    Look in every `.rs` in the util's `src/` directory.
    //
    // Collect both: the optional `options` mod (if present) and any
    // bare-name `OPT_*` / `ARG_*` constants we should also emit so
    // uu_app's bare-name references resolve.
    let mut options_mod = find_mod(&file, "options").cloned();
    if options_mod.is_none() {
        options_mod = find_mod_in_sibling_files(&src_path, "options")?;
    }
    let bare_name_consts = collect_option_constants(&file);
    if options_mod.is_none() && bare_name_consts.is_empty() {
        bail!(
            "could not find `mod options` or any module-level `OPT_*`/`ARG_*` \
             constants in {} or its sibling files",
            src_path.display()
        );
    }
    let mut uu_app = find_fn(&file, "uu_app")
        .expect("uu_app presence already verified in source resolution above")
        .clone();

    // Harvest `.env(...)` annotations BEFORE the rewriter strips them
    // from the runtime Arg chain. Each entry becomes a row in the
    // generated `<UTIL>_ENV_DEFAULTS` table that the bashkit-side shim
    // (`builtins::clap_env::apply_env_defaults`) consults to fold
    // bashkit's virtual `ctx.env` into clap parsing — never the host
    // `std::env` (TM-INF-024).
    let env_defaults = collect_env_defaults(&uu_app)?;

    let mut rw = Rewriter {
        translations,
        unresolved: vec![],
    };
    rw.visit_item_fn_mut(&mut uu_app);
    if !rw.unresolved.is_empty() {
        bail!(
            "unresolved translate!() keys (no en-US.ftl entry): {:?}",
            rw.unresolved
        );
    }

    // Some uu_app() definitions reference free helpers from the same
    // source file: free functions (e.g. shuf's `parse_range`, used as
    // a clap `value_parser`) or custom value-parser structs together
    // with their `impl` blocks (e.g. mktemp's `OptionalPathBufParser`,
    // realpath's `NonEmptyOsStringParser`). Inline both kinds so the
    // generated file compiles standalone, running them through the
    // same rewriter so any embedded `translate!()` calls resolve too.
    let mut helpers = collect_referenced_items(&uu_app, &file);
    for item in &mut helpers {
        rw.visit_item_mut(item);
    }
    if !rw.unresolved.is_empty() {
        bail!(
            "unresolved translate!() keys in inlined helpers: {:?}",
            rw.unresolved
        );
    }

    validate_uu_app_body(&uu_app)?;

    let cmd_fn_name = syn::Ident::new(&format!("{util}_command"), proc_macro2::Span::call_site());
    uu_app.sig.ident = cmd_fn_name.clone();
    let uu_app_block = &uu_app.block;
    let uu_app_sig = &uu_app.sig;
    let uu_app_attrs = &uu_app.attrs;

    let header_comment = format!(
        "// GENERATED by bashkit-coreutils-port. DO NOT EDIT.\n\
         //\n\
         // Source: uutils/coreutils@{rev} src/uu/{util}/\n\
         // Regenerate: cargo run -p bashkit-coreutils-port -- <UUTILS_DIR> {util} <REV>\n\
         //\n\
         // Original uutils licensed MIT; see THIRD_PARTY_LICENSES.\n\n",
    );

    // Optional `mod options { ... }` (cat-style); collapses to nothing
    // for utils that use bare-name constants.
    let has_options_mod = options_mod.is_some();
    let options_mod_tokens: TokenStream = match options_mod {
        Some(m) => quote!(#m),
        None => quote!(),
    };
    // Bare-name constants for utils that don't wrap them in a mod.
    let const_tokens: Vec<TokenStream> = bare_name_consts
        .into_iter()
        .map(|c| quote::quote!(#c))
        .collect();
    // Only re-export `options::*` when there is an `options` mod to glob.
    // utils that put constants at module level (mktemp, realpath, ...)
    // already have them in scope by name.
    let options_glob: TokenStream = if has_options_mod {
        quote! {
            #[allow(unused_imports)]
            use options::*;
        }
    } else {
        quote!()
    };

    // `<UTIL>_ENV_DEFAULTS` table — the codegen-side half of the
    // virtual-env shim. Always emitted (possibly empty) so every
    // generated module exposes the same surface; bashkit-side
    // builtins import it by name and feed it to
    // `crate::builtins::clap_env::apply_env_defaults`.
    let env_defaults_const_name = syn::Ident::new(
        &format!("{}_ENV_DEFAULTS", util.to_uppercase()),
        proc_macro2::Span::call_site(),
    );
    let env_defaults_rows: Vec<TokenStream> = env_defaults
        .iter()
        .map(|d| {
            let arg_id = &d.arg_id;
            let long = &d.long;
            let env_var = LitStr::new(&d.env_var, proc_macro2::Span::call_site());
            let kind_ident = syn::Ident::new(d.kind.as_str(), proc_macro2::Span::call_site());
            quote! {
                crate::builtins::clap_env::EnvDefault {
                    arg_id: #arg_id,
                    long: #long,
                    env_var: #env_var,
                    kind: crate::builtins::clap_env::EnvKind::#kind_ident,
                }
            }
        })
        .collect();
    let env_defaults_tokens: TokenStream = quote! {
        /// Sidecar harvest of every `Arg::env(...)` annotation the codegen
        /// stripped from the runtime Arg chain (TM-INF-024). Consumed by
        /// `crate::builtins::clap_env::apply_env_defaults` so bashkit's
        /// virtual `ctx.env` — never `std::env` — drives clap's env-default
        /// path. Order matches the chain order in the original `uu_app()`.
        pub static #env_defaults_const_name: &[crate::builtins::clap_env::EnvDefault] = &[
            #(#env_defaults_rows),*
        ];
    };

    let body: TokenStream = quote! {
        #![allow(unused_imports, dead_code)]

        // Always import the broader clap+std surface a few utils need.
        // `#![allow(unused_imports)]` above silences warnings for
        // utilities that don't reach for them. Add to this list when a
        // newly-ported util needs another std type that the inlined
        // helpers reference (e.g. shuf's `parse_range` returns
        // `RangeInclusive<u64>`).
        use clap::builder::{
            NonEmptyStringValueParser, PossibleValue, PossibleValuesParser, TypedValueParser,
            ValueParser, ValueParserFactory,
        };
        use clap::{Arg, ArgAction, Command};
        use std::ffi::{OsStr, OsString};
        use std::ops::RangeInclusive;
        use std::path::PathBuf;
        use std::str::FromStr;

        #options_mod_tokens
        #(#const_tokens)*

        #options_glob

        /// Vendored stand-in for `uucore::format_usage`.
        ///
        /// Upstream wraps the usage line with stylized "Usage:" prefix logic
        /// driven by uucore's locale stack. For our purposes the raw string
        /// is enough; clap's `override_usage` accepts the literal as-is.
        fn format_usage(s: &str) -> String {
            s.to_string()
        }

        #env_defaults_tokens

        // Inlined free-function helpers referenced by `uu_app()` (e.g.
        // shuf's `parse_range` as a clap `value_parser`). Copied
        // verbatim from the source file with the same translate!()
        // rewriting applied so they compile without uucore.
        #(#helpers)*

        #(#uu_app_attrs)*
        pub #uu_app_sig #uu_app_block
    };

    let parsed: syn::File = syn::parse2(body).context("synthesize generated file")?;
    let pretty = prettyplease::unparse(&parsed);
    Ok(format!("{header_comment}{pretty}"))
}

/// Parse a Fluent file restricted to the `key = value` and continuation
/// subset that uutils uses for help/about strings.
///
/// Supported:
///   `key = value`
///   `key = value`
///   `  continuation line`
///
/// Rejected (silently skipped, not used by argument help):
///   message references `{ -brand }`, selectors, plurals.
fn parse_ftl(src: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_val: String = String::new();

    let flush = |out: &mut HashMap<String, String>, key: &mut Option<String>, val: &mut String| {
        if let Some(k) = key.take() {
            out.insert(k, std::mem::take(val).trim_end().to_string());
        }
    };

    for raw_line in src.lines() {
        let line = raw_line;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            flush(&mut out, &mut current_key, &mut current_val);
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            // continuation
            if current_key.is_some() {
                current_val.push('\n');
                current_val.push_str(line.trim_start());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            flush(&mut out, &mut current_key, &mut current_val);
            current_key = Some(k.trim().to_string());
            current_val = v.trim().to_string();
        }
    }
    flush(&mut out, &mut current_key, &mut current_val);
    out
}

/// One row in the codegen's harvest of `Arg::env(...)` annotations.
/// Carries token-stream-form expressions for `arg_id` and `long` so we
/// can quote them straight back into the generated `<UTIL>_ENV_DEFAULTS`
/// table without resolving constants — they evaluate to `&'static str`
/// in the generated module's scope (uutils' `mod options` items are
/// `pub static FOO: &str = "..."`).
struct EnvDefaultMeta {
    arg_id: TokenStream,
    long: TokenStream,
    env_var: String,
    kind: EnvKindMeta,
}

#[derive(Clone, Copy)]
enum EnvKindMeta {
    Single,
    Bool,
    Multi,
}

impl EnvKindMeta {
    fn as_str(self) -> &'static str {
        match self {
            EnvKindMeta::Single => "Single",
            EnvKindMeta::Bool => "Bool",
            EnvKindMeta::Multi => "Multi",
        }
    }
}

/// Walk `uu_app()` body, find every `.arg(<chain>)` whose `<chain>`
/// contains `.env("VAR")`, and harvest one `EnvDefaultMeta` per chain.
///
/// Each chain is rooted at `Arg::new(<id_expr>)` — innermost receiver
/// of the method-call stack. We extract:
///   - `arg_id`: the expression `Arg::new` was called with.
///   - `long`: the expression `.long(...)` was called with (required;
///     bail if absent — every uutils env-bound option has `.long`).
///   - `env_var`: the string literal `.env(...)` was called with.
///   - `kind`: `Bool` if `.action(ArgAction::SetTrue|SetFalse)` is
///     present, `Multi` if `.value_delimiter(...)` is present, else
///     `Single` (the dominant case for uutils — `TIME_STYLE`,
///     `TABSIZE`, `BLOCK_SIZE`, …).
///
/// Run BEFORE the rewriter mutates `uu_app`, while `.env(...)` is still
/// in place. The rewriter then strips the call from the runtime chain.
fn collect_env_defaults(uu_app: &ItemFn) -> Result<Vec<EnvDefaultMeta>> {
    use syn::visit::{self, Visit};

    struct Collector {
        out: Vec<EnvDefaultMeta>,
        errors: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Collector {
        fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
            // Match `.arg(<single-expr>)` — every Arg chain in
            // `Command::new(...).arg(<chain>)…` shows up here.
            if mc.method == "arg"
                && mc.args.len() == 1
                && let Some(arg_expr) = mc.args.first()
                && let Some(meta) = parse_arg_chain_for_env(arg_expr, &mut self.errors)
            {
                self.out.push(meta);
            }
            visit::visit_expr_method_call(self, mc);
        }
    }

    let mut c = Collector {
        out: vec![],
        errors: vec![],
    };
    c.visit_item_fn(uu_app);
    if !c.errors.is_empty() {
        bail!("env-default harvest errors: {}", c.errors.join("; "));
    }
    Ok(c.out)
}

/// For an `.arg(<expr>)` argument, walk the method-call chain and
/// extract env-default metadata if `.env(...)` is present. Returns
/// `None` for chains that don't carry a `.env(...)` annotation.
fn parse_arg_chain_for_env(arg_expr: &Expr, errors: &mut Vec<String>) -> Option<EnvDefaultMeta> {
    let mut node: &Expr = arg_expr;
    let mut env_var: Option<String> = None;
    let mut long: Option<TokenStream> = None;
    let mut kind = EnvKindMeta::Single;

    while let Expr::MethodCall(mc) = node {
        let m = mc.method.to_string();
        if m == "env" {
            if let Some(Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            })) = mc.args.first()
            {
                env_var = Some(s.value());
            } else {
                errors.push(format!(
                    "Arg::env(...) call with non-string-literal argument: {}",
                    quote::quote!(#mc)
                ));
            }
        } else if m == "long" && mc.args.len() == 1 {
            let arg = &mc.args[0];
            long = Some(quote!(#arg));
        } else if m == "action"
            && mc
                .args
                .first()
                .is_some_and(expr_is_set_true_or_set_false_action)
        {
            kind = EnvKindMeta::Bool;
        } else if m == "value_delimiter" {
            kind = EnvKindMeta::Multi;
        }
        node = &mc.receiver;
    }

    // Bottom of the chain must be `Arg::new(<id_expr>)` — record the id.
    let arg_id = match node {
        Expr::Call(call) => {
            if !path_ends_with_arg_new(&call.func) {
                return None;
            }
            match call.args.first() {
                Some(id_expr) => quote!(#id_expr),
                None => {
                    errors.push("Arg::new() with zero arguments".into());
                    return None;
                }
            }
        }
        _ => return None,
    };

    let env_var = env_var?;
    let long = match long {
        Some(l) => l,
        None => {
            errors.push(format!(
                "Arg with .env({env_var:?}) but no .long(...) — bashkit's \
                 virtual-env shim needs a long flag to inject"
            ));
            return None;
        }
    };

    Some(EnvDefaultMeta {
        arg_id,
        long,
        env_var,
        kind,
    })
}

fn expr_is_set_true_or_set_false_action(expr: &Expr) -> bool {
    let Expr::Path(p) = expr else {
        return false;
    };
    let segs = path_segments(&p.path);
    let last = segs.last().map(String::as_str).unwrap_or("");
    matches!(last, "SetTrue" | "SetFalse")
}

fn path_ends_with_arg_new(func: &Expr) -> bool {
    let Expr::Path(p) = func else {
        return false;
    };
    let segs = path_segments(&p.path);
    let last_two: Vec<&str> = segs
        .iter()
        .rev()
        .take(2)
        .map(String::as_str)
        .collect::<Vec<_>>();
    matches!(last_two.as_slice(), ["new", "Arg"])
}

/// Walk `uu_app()`'s body looking for plain identifier references
/// (single-segment paths) and return any matching free items defined
/// at the top level of the source file. Handles two helper shapes:
///
/// * Free functions used as `value_parser`s (e.g. shuf's `parse_range`).
/// * Newtype `value_parser` structs with their `impl` blocks (e.g.
///   mktemp's `OptionalPathBufParser`, realpath's
///   `NonEmptyOsStringParser`). Both `impl TypedValueParser` and
///   `impl ValueParserFactory` blocks for the struct are pulled in.
///
/// Skips items already provided by the generated preamble
/// (e.g. `format_usage`). One-level only: a helper that itself
/// references another local helper surfaces as a compile error rather
/// than triggering recursive inlining. Broaden when a real case demands
/// it.
fn collect_referenced_items(uu_app: &ItemFn, file: &syn::File) -> Vec<Item> {
    use syn::visit::{self, Visit};

    struct IdentCollector(std::collections::HashSet<String>);
    impl<'ast> Visit<'ast> for IdentCollector {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if path.segments.len() == 1
                && let Some(seg) = path.segments.first()
            {
                self.0.insert(seg.ident.to_string());
            }
            visit::visit_path(self, path);
        }
    }

    let mut idents = IdentCollector(Default::default());
    idents.visit_item_fn(uu_app);
    let names = idents.0;

    const PROVIDED_BY_PREAMBLE: &[&str] = &["format_usage"];

    let mut out: Vec<Item> = Vec::new();
    let mut struct_names: std::collections::HashSet<String> = Default::default();
    for item in &file.items {
        match item {
            Item::Fn(f) => {
                let name = f.sig.ident.to_string();
                if names.contains(&name) && !PROVIDED_BY_PREAMBLE.contains(&name.as_str()) {
                    out.push(Item::Fn(f.clone()));
                }
            }
            Item::Struct(s) => {
                let name = s.ident.to_string();
                if names.contains(&name) {
                    out.push(Item::Struct(s.clone()));
                    struct_names.insert(name);
                }
            }
            _ => {}
        }
    }
    // Pull in `impl ... for <Struct>` and inherent `impl <Struct>`
    // blocks for any struct we just inlined, preserving source order so
    // `impl` blocks always follow their struct definition.
    for item in &file.items {
        if let Item::Impl(im) = item
            && let Some(target) = impl_target_ident(im)
            && struct_names.contains(&target)
        {
            out.push(Item::Impl(im.clone()));
        }
    }
    out
}

/// Extract the single-segment ident of an `impl` block's `Self` type.
/// Returns `None` for impls on generic or path-qualified types — the
/// helper inlining path only handles the simple newtype shape uutils
/// uses for value-parser structs.
fn impl_target_ident(im: &ItemImpl) -> Option<String> {
    if let Type::Path(tp) = im.self_ty.as_ref()
        && tp.qself.is_none()
        && tp.path.segments.len() == 1
    {
        return Some(tp.path.segments[0].ident.to_string());
    }
    None
}

fn find_mod<'a>(file: &'a syn::File, name: &str) -> Option<&'a ItemMod> {
    file.items.iter().find_map(|it| match it {
        Item::Mod(m) if m.ident == name => Some(m),
        _ => None,
    })
}

/// Look for `mod <name>` (or `pub mod <name>`) in any `.rs` file that lives
/// alongside `src_path`. Used by utilities that declare their option-key
/// module in a sibling file (e.g. `ls/src/config.rs::pub mod options`)
/// and `use` it from `<util>.rs`. Returns the first match — the codegen
/// tool already requires that exactly one `mod options` exists per util.
fn find_mod_in_sibling_files(src_path: &Path, name: &str) -> Result<Option<ItemMod>> {
    let parent = src_path
        .parent()
        .ok_or_else(|| anyhow!("source path has no parent: {}", src_path.display()))?;
    let entries = std::fs::read_dir(parent)
        .with_context(|| format!("scan sibling files in {}", parent.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        // Skip the file we already searched and anything that isn't .rs.
        if path == src_path {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("read sibling {}", path.display()))?;
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(_) => continue, // sibling that doesn't parse — leave it alone
        };
        if let Some(m) = find_mod(&file, name) {
            return Ok(Some(m.clone()));
        }
    }
    Ok(None)
}

/// Collect module-level `const OPT_FOO: &str = ...` / `static OPT_FOO`
/// declarations that some uutils sources use in place of a
/// `mod options { ... }`. uu_app() bodies in these utils refer to the
/// constants by their bare name (`Arg::new(OPT_FOO)`); we emit them
/// at the same scope in the generated file so the bare-name references
/// resolve unchanged.
///
/// Filter: name must start with `OPT_` or `ARG_` to avoid sweeping in
/// unrelated source-level constants.
fn collect_option_constants(file: &syn::File) -> Vec<Item> {
    file.items
        .iter()
        .filter(|it| match it {
            Item::Const(c) => {
                let n = c.ident.to_string();
                n.starts_with("OPT_") || n.starts_with("ARG_")
            }
            Item::Static(s) => {
                let n = s.ident.to_string();
                n.starts_with("OPT_") || n.starts_with("ARG_")
            }
            _ => false,
        })
        .cloned()
        .collect()
}

// THREAT[TM-INF-025]: args-mode consumes third-party Rust from uutils.
// Only clap builder expressions may become executable bashkit code;
// any other statement shape would run during CI/parser construction.
//
// Two accepted body shapes:
//   1. `<Command::new(...) chain>`  (single tail expression)
//   2. `let <ident> = <Command::new(...) chain>;
//       <ident>.<builder method>(...)...`
//      — equivalent to a single chain split across a binding. The
//      tail's innermost receiver must be the let-bound identifier;
//      both halves run through the same chain validator. The binding is
//      deliberately plain: no `mut`, `ref`, type ascription, subpattern,
//      non-doc attributes, or `let ... else`.
fn validate_uu_app_body(uu_app: &ItemFn) -> Result<()> {
    match uu_app.block.stmts.as_slice() {
        [Stmt::Expr(expr, None)] => validate_command_chain_expr(expr),
        [Stmt::Local(local), Stmt::Expr(tail_expr, None)] => {
            validate_let_bound_command_body(local, tail_expr)
        }
        stmts => bail!(
            "unsafe uu_app body: expected a single clap Command builder expression \
             or `let <ident> = Command::new(...)...; <ident>.method(...)...`, \
             found {} statements",
            stmts.len()
        ),
    }
}

fn validate_command_chain_expr(expr: &Expr) -> Result<()> {
    let mut validator = UuAppExprValidator { errors: vec![] };
    syn::visit::Visit::visit_expr(&mut validator, expr);
    if !validator.errors.is_empty() {
        bail!("unsafe uu_app body: {}", validator.errors.join("; "));
    }
    if !chain_roots_at_command_new(expr) {
        bail!(
            "unsafe uu_app body: tail expression must be a clap::Command::new(...) builder chain"
        );
    }
    Ok(())
}

fn validate_let_bound_command_body(local: &syn::Local, tail_expr: &Expr) -> Result<()> {
    let binding_ident = plain_let_ident(local)?;
    let init = local
        .init
        .as_ref()
        .ok_or_else(|| anyhow!("unsafe uu_app body: let binding must have an initializer"))?;
    if init.diverge.is_some() {
        bail!("unsafe uu_app body: let-else is not allowed");
    }
    // The bound expression must itself be a Command::new(...) builder
    // chain so the only thing the binding can hold is a clap Command.
    validate_command_chain_expr(&init.expr)?;

    // The tail must be a method chain whose innermost receiver is the
    // let-bound identifier — i.e. structurally a continuation of the
    // builder chain.
    let mut validator = UuAppExprValidator { errors: vec![] };
    syn::visit::Visit::visit_expr(&mut validator, tail_expr);
    if !validator.errors.is_empty() {
        bail!("unsafe uu_app body: {}", validator.errors.join("; "));
    }
    if !tail_chains_back_to_ident(tail_expr, binding_ident) {
        bail!(
            "unsafe uu_app body: tail expression must be a `{}.method(...)...` \
             chain on the let binding",
            binding_ident
        );
    }
    Ok(())
}

/// Returns the bound identifier of a plain `let <ident> = ...;`.
/// Rejects destructuring, `mut`, `ref`, type ascription, subpatterns,
/// or non-doc attributes — anything that could hide behavior in the
/// binding pattern or weaken the no-mutation proof.
fn plain_let_ident(local: &syn::Local) -> Result<&syn::Ident> {
    if local.attrs.iter().any(|attr| !attr.path().is_ident("doc")) {
        bail!("unsafe uu_app body: let binding must not carry non-doc attributes");
    }
    match &local.pat {
        syn::Pat::Ident(pi) => {
            if !pi.attrs.is_empty()
                || pi.by_ref.is_some()
                || pi.mutability.is_some()
                || pi.subpat.is_some()
            {
                bail!(
                    "unsafe uu_app body: let binding must be plain `let <ident> = ...` \
                     (no `mut`, no `ref`, no subpattern)"
                );
            }
            Ok(&pi.ident)
        }
        syn::Pat::Type(_) => bail!("unsafe uu_app body: let binding must not use type ascription"),
        _ => bail!(
            "unsafe uu_app body: let binding must bind a single identifier, \
             not a destructuring pattern"
        ),
    }
}

/// Walk down method-call receivers and require the innermost expression
/// to be a bare reference to `target` — i.e. the tail is shaped like
/// `target.foo(...).bar(...)`. Anything else (a fresh `Command::new`,
/// a different binding, a function call) is rejected.
fn tail_chains_back_to_ident(expr: &Expr, target: &syn::Ident) -> bool {
    let mut cur = expr;
    loop {
        match cur {
            Expr::MethodCall(mc) => cur = &mc.receiver,
            Expr::Path(p)
                if p.qself.is_none()
                    && p.path.leading_colon.is_none()
                    && p.path.segments.len() == 1
                    && p.path.segments[0].ident == *target
                    && matches!(p.path.segments[0].arguments, syn::PathArguments::None) =>
            {
                return true;
            }
            _ => return false,
        }
    }
}

struct UuAppExprValidator {
    errors: Vec<String>,
}

impl<'ast> syn::visit::Visit<'ast> for UuAppExprValidator {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if is_disallowed_chain_method(&node.method.to_string()) {
            self.errors.push(format!(
                "method is not allowed in command builder: {}",
                quote::quote!(#node)
            ));
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        self.errors.push(format!(
            "closure is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        if let Err(err) = validate_allowed_command_builder_macro(&node.mac) {
            self.errors.push(format!("{err}: {}", quote::quote!(#node)));
        }
        syn::visit::visit_expr_macro(self, node);
    }
    fn visit_expr_block(&mut self, node: &'ast syn::ExprBlock) {
        self.errors.push(format!(
            "block expression is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.errors.push(format!(
            "unsafe block is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
        self.errors.push(format!(
            "async block is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.errors.push(format!(
            "loop is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.errors.push(format!(
            "while is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.errors.push(format!(
            "for loop is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        self.errors.push(format!(
            "match is not allowed in command builder: {}",
            quote::quote!(#node)
        ));
    }
}

fn chain_roots_at_command_new(mut expr: &Expr) -> bool {
    while let Expr::MethodCall(mc) = expr {
        expr = &mc.receiver;
    }

    let Expr::Call(call) = expr else {
        return false;
    };
    path_ends_with_command_new(&call.func)
}

fn path_ends_with_command_new(func: &Expr) -> bool {
    let Expr::Path(p) = func else {
        return false;
    };
    let segs = path_segments(&p.path);
    matches!(segs.as_slice(), [single, new] if single == "Command" && new == "new")
        || matches!(segs.as_slice(), [clap, command, new] if clap == "clap" && command == "Command" && new == "new")
}

fn validate_allowed_command_builder_macro(mac: &syn::Macro) -> Result<()> {
    // Only fully qualified trusted builder macros may cross this boundary:
    // unqualified macros can be shadowed by copied uutils modules before codegen.
    let segs = path_segments(&mac.path);
    if matches!(segs.as_slice(), [env] if env == "env") {
        return validate_env_macro(mac);
    }
    if matches!(segs.as_slice(), [clap, value_parser] if clap == "clap" && value_parser == "value_parser")
    {
        return validate_value_parser_macro(mac);
    }
    bail!("macro is not allowed in command builder");
}

fn validate_env_macro(mac: &syn::Macro) -> Result<()> {
    let lit: LitStr = syn::parse2(mac.tokens.clone())
        .context("env! in command builder must be env!(\"CARGO_PKG_VERSION\")")?;
    if lit.value() != "CARGO_PKG_VERSION" {
        bail!("env! in command builder only allows CARGO_PKG_VERSION");
    }
    Ok(())
}

fn validate_value_parser_macro(mac: &syn::Macro) -> Result<()> {
    // syn::Type accepts `Type::Macro`, so a bare `parse2::<Type>` lets
    // `value_parser!(env!("…"))` (or any nested macro) slip through. Reject
    // macro-typed payloads outright — only plain type paths/refs are valid
    // value_parser! arguments and keeping the surface narrow preserves
    // TM-INF-025's defence-in-depth posture against compile-time secret leaks.
    let ty: syn::Type = syn::parse2(mac.tokens.clone())
        .context("value_parser! in command builder must contain a type path")?;
    if matches!(ty, syn::Type::Macro(_)) {
        bail!("value_parser! in command builder must not contain a nested macro");
    }
    Ok(())
}

fn is_disallowed_chain_method(method: &str) -> bool {
    matches!(
        method,
        "spawn"
            | "status"
            | "output"
            | "exec"
            | "wait"
            | "kill"
            | "map"
            | "and_then"
            | "or_else"
            | "unwrap"
            | "expect"
    )
}

fn find_fn<'a>(file: &'a syn::File, name: &str) -> Option<&'a ItemFn> {
    file.items.iter().find_map(|it| match it {
        Item::Fn(f) if f.sig.ident == name => Some(f),
        _ => None,
    })
}

/// Look for `fn <name>` (or `pub fn <name>`) in any sibling `.rs` file.
/// Used by utilities that split `uu_app()` into a sibling `cli.rs` and
/// re-export it from `<util>.rs` (e.g. tee). Returns the first match —
/// when uu_app is defined in a sibling, that file becomes the source
/// of truth for option constants and helper inlining as well.
fn find_sibling_file_with_fn(src_path: &Path, name: &str) -> Result<Option<(PathBuf, syn::File)>> {
    let parent = src_path
        .parent()
        .ok_or_else(|| anyhow!("source path has no parent: {}", src_path.display()))?;
    let entries = std::fs::read_dir(parent)
        .with_context(|| format!("scan sibling files in {}", parent.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path == src_path {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("read sibling {}", path.display()))?;
        let file = match syn::parse_file(&src) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if find_fn(&file, name).is_some() {
            return Ok(Some((path, file)));
        }
    }
    Ok(None)
}

struct Rewriter {
    translations: HashMap<String, String>,
    unresolved: Vec<String>,
}

impl Rewriter {
    fn rewrite_macro(&mut self, m: &ExprMacro) -> Option<Expr> {
        let path = path_segments(&m.mac.path);
        let last = path.last().map(String::as_str).unwrap_or("");

        if last == "translate" {
            // translate!("key") or translate!("key", "var" => expr, ...)
            // We only handle the simple-key case; if interpolation args are
            // present we leave the call alone (caller tracks unresolved).
            let tokens = m.mac.tokens.clone();
            let parsed: syn::Result<TranslateArgs> = syn::parse2(tokens);
            match parsed {
                Ok(TranslateArgs::Simple(key)) => {
                    let key_str = key.value();
                    if let Some(val) = self.translations.get(&key_str) {
                        let lit = LitStr::new(val, key.span());
                        return Some(parse_quote!(::std::string::String::from(#lit)));
                    } else {
                        self.unresolved.push(key_str);
                    }
                }
                Ok(TranslateArgs::Complex) => {
                    // arg-interpolating translate!() — unexpected in uu_app(),
                    // but if we hit one we surface it for manual handling.
                    self.unresolved
                        .push(format!("(complex translate at {:?})", m.mac.path));
                }
                Err(_) => {}
            }
            return None;
        }

        if last == "crate_version" && path_starts_with(&path, "uucore") {
            return Some(parse_quote!(env!("CARGO_PKG_VERSION")));
        }
        None
    }

    fn rewrite_call(&mut self, call: &mut ExprCall) -> Option<Expr> {
        let func_path = match &*call.func {
            Expr::Path(p) => path_segments(&p.path),
            _ => return None,
        };
        let last = func_path.last().map(String::as_str).unwrap_or("");

        if last == "format_usage" && path_starts_with(&func_path, "uucore") {
            // uucore::format_usage(x)  ->  format_usage(x)
            if let Expr::Path(p) = &mut *call.func {
                p.path = parse_quote!(format_usage);
            }
            return None; // descend normally; we only retargeted the path
        }

        if last == "localized_help_template" && path_starts_with(&func_path, "uucore") {
            // Drop the call: clap's default template is fine for now.
            // Returning a sentinel marker lets the caller (visit_expr_mut on
            // the surrounding MethodCall) elide the chained step.
            return Some(parse_quote!(__bashkit_drop_chain__()));
        }

        if last == "configure_localized_command" && path_starts_with(&func_path, "uucore") {
            // uucore::clap_localization::configure_localized_command(cmd)
            // -> cmd. The wrapper pulls Fluent into the Command's
            // help/version paths; bashkit's Command works without it.
            if let Some(first) = call.args.first() {
                return Some(first.clone());
            }
        }

        if last == "new" && (matches_shortcut_value_parser(&func_path)) {
            // ShortcutValueParser::new([...]) -> PossibleValuesParser::new([...]).
            // uucore's parser permits unambiguous abbreviation; we trade
            // that for clap's exact-match semantics. The accepted-value
            // set is identical, only the abbreviation behavior differs —
            // documented as out-of-scope by #1531.
            if let Expr::Path(p) = &mut *call.func {
                p.path = parse_quote!(::clap::builder::PossibleValuesParser::new);
            }
            return None;
        }

        None
    }
}

impl VisitMut for Rewriter {
    fn visit_expr_mut(&mut self, node: &mut Expr) {
        // First, descend so inner replacements happen before we inspect the
        // outer node (e.g., method-call chains where the receiver is itself
        // a method call we may need to elide).
        visit_mut::visit_expr_mut(self, node);

        // Rewrite leaf macros and calls.
        if let Expr::Macro(m) = node
            && let Some(replacement) = self.rewrite_macro(m)
        {
            *node = replacement;
            return;
        }
        if let Expr::Call(c) = node
            && let Some(replacement) = self.rewrite_call(c)
        {
            *node = replacement;
            return;
        }

        // Elide chained method calls whose argument list reduced to the
        // dropped sentinel. e.g., `.help_template(__bashkit_drop_chain__())`
        // collapses to its receiver.
        if let Expr::MethodCall(mc) = node
            && mc.args.len() == 1
            && matches!(mc.args.first(), Some(Expr::Call(c)) if is_drop_sentinel(c))
        {
            let receiver = (*mc.receiver).clone();
            *node = receiver;
            return;
        }

        // Strip `clap::Arg::env(...)` chained method calls. uutils sources
        // pull defaults like `TABSIZE`/`TIME_STYLE` from `std::env`, but
        // bashkit sandboxes scripts inside its own `ctx.env`. Letting clap
        // read from the host process leaks host state into the sandbox
        // (TM-INF-024): scripts can probe whether the host has these vars
        // set, and a host-set `TIME_STYLE` would tunnel a value through
        // bashkit's "unsupported option" gate and kill `ls` for unrelated
        // tenants. Drop the `.env(...)` step from the Arg chain at codegen
        // time so generated parsers only see argv. Conservative match:
        // method ident is `env` and the single argument is a string
        // literal (the only shape uutils uses).
        if let Expr::MethodCall(mc) = node
            && mc.method == "env"
            && mc.args.len() == 1
            && matches!(
                mc.args.first(),
                Some(Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(_),
                    ..
                }))
            )
        {
            let receiver = (*mc.receiver).clone();
            *node = receiver;
        }
    }
}

fn is_drop_sentinel(c: &ExprCall) -> bool {
    if let Expr::Path(p) = &*c.func {
        let segs = path_segments(&p.path);
        return segs.len() == 1 && segs[0] == "__bashkit_drop_chain__";
    }
    false
}

enum TranslateArgs {
    Simple(LitStr),
    Complex,
}

impl syn::parse::Parse for TranslateArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let key: LitStr = input.parse()?;
        if input.is_empty() {
            return Ok(TranslateArgs::Simple(key));
        }
        Ok(TranslateArgs::Complex)
    }
}

fn path_segments(p: &syn::Path) -> Vec<String> {
    p.segments.iter().map(|s| s.ident.to_string()).collect()
}

fn path_starts_with(segs: &[String], head: &str) -> bool {
    segs.first().map(String::as_str) == Some(head)
}

/// Return `true` when the call path resolves to uucore's
/// `ShortcutValueParser::new`, regardless of how the caller spelled the
/// import. uutils sources reach the type via several aliases:
///
/// - `ShortcutValueParser::new(...)` (after `use uucore::parser::shortcut_value_parser::ShortcutValueParser`)
/// - `parser::shortcut_value_parser::ShortcutValueParser::new(...)`
/// - the fully qualified `uucore::parser::shortcut_value_parser::ShortcutValueParser::new(...)`
fn matches_shortcut_value_parser(segs: &[String]) -> bool {
    let last_two: Vec<&str> = segs
        .iter()
        .rev()
        .take(2)
        .map(String::as_str)
        .collect::<Vec<_>>();
    matches!(last_two.as_slice(), ["new", "ShortcutValueParser"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let uutils = tmp.path().join("uutils");
        for (rel, content) in files {
            let path = uutils.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        (tmp, uutils)
    }

    #[test]
    fn rejects_executable_statements_in_uu_app() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    std::fs::write("coreutils-port-poc", b"owned").unwrap();
    std::process::abort();
    Command::new("cat").arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unsafe uu_app"), "got: {msg}");
    }

    #[test]
    fn accepts_single_command_builder_expression() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat").arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let body = run(&uutils, "cat", "poc").unwrap();
        assert!(body.contains("pub fn cat_command() -> clap::Command"));
        assert!(body.contains("Command::new(\"cat\")"));
    }

    #[test]
    fn rejects_non_clap_command_root_chain() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    std::process::Command::new("sh")
        .arg("-c")
        .arg("echo nope")
        .status()
        .map(|_| clap::Command::new("cat").arg(clap::Arg::new(options::FILE)))
        .unwrap()
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unsafe uu_app"), "got: {msg}");
    }

    #[test]
    fn accepts_expected_builder_macros() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat")
        .version(uucore::crate_version!())
        .arg(Arg::new(options::FILE).value_parser(clap::value_parser!(std::ffi::OsString)))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let body = run(&uutils, "cat", "poc").unwrap();
        assert!(
            body.contains("version(env!(\"CARGO_PKG_VERSION\"))"),
            "got: {body}"
        );
        assert!(
            body.contains("value_parser(clap::value_parser!(std::ffi::OsString))"),
            "got: {body}"
        );
    }

    #[test]
    fn rejects_non_pkg_version_env_macro() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat").version(env!("CI_SECRET"))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("env! in command builder only allows CARGO_PKG_VERSION"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_env_macro_with_nested_macro_tokens() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat").version(env!(include_str!("/etc/passwd")))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("env! in command builder must be env!(\"CARGO_PKG_VERSION\")"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_value_parser_macro_with_nested_macro_tokens() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat").arg(
        Arg::new(options::FILE).value_parser(clap::value_parser!(env!("CI_SECRET"))),
    )
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("value_parser! in command builder must not contain a nested macro"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_unqualified_value_parser_macro() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat").arg(
        Arg::new(options::FILE).value_parser(value_parser!(std::ffi::OsString)),
    )
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("macro is not allowed in command builder"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_unexpected_macro_in_builder_chain() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    Command::new("cat").arg(Arg::new(options::FILE).help(format!("{}", "x")))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("macro is not allowed in command builder"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_non_command_tail_expression() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    std::process::abort()
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Command::new"), "got: {msg}");
    }

    #[test]
    fn accepts_let_bound_command_with_tail_chain() {
        // Upstream uutils truncate now splits uu_app() into
        // `let cmd = Command::new(...)...;
        //  configure_localized_command(cmd).arg(...)...`.
        // The Rewriter folds `configure_localized_command(cmd)` to `cmd`,
        // leaving exactly the `let`-then-tail shape the validator must accept.
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    let cmd = Command::new("cat")
        .version(uucore::crate_version!())
        .infer_long_args(true);
    uucore::clap_localization::configure_localized_command(cmd)
        .arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let body = run(&uutils, "cat", "poc").unwrap();
        assert!(body.contains("pub fn cat_command() -> clap::Command"));
        assert!(body.contains("Command::new(\"cat\")"));
        // The configure_localized_command(cmd) wrapper is folded away;
        // only `cmd.arg(...)` remains.
        assert!(
            !body.contains("configure_localized_command"),
            "wrapper should be stripped: {body}"
        );
        assert!(body.contains("cmd.arg("), "got: {body}");
    }

    #[test]
    fn rejects_let_mut_binding_in_uu_app() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    let mut cmd = Command::new("cat");
    cmd.arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no `mut`"), "got: {msg}");
    }

    #[test]
    fn rejects_let_with_non_command_initializer() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    let cmd = std::process::Command::new("sh").arg("-c").arg("nope").status().unwrap();
    Command::new("cat").arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unsafe uu_app"), "got: {msg}");
    }

    #[test]
    fn rejects_let_init_not_rooted_at_command_new() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    let cmd = some::factory();
    cmd.arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Command::new"), "got: {msg}");
    }

    #[test]
    fn rejects_tail_not_chained_off_let_binding() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    let cmd = Command::new("cat");
    Command::new("cat").arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("chain on the let binding"), "got: {msg}");
    }

    #[test]
    fn rejects_three_statement_body() {
        let (_tmp, uutils) = fixture(&[
            (
                "src/uu/cat/src/cat.rs",
                r#"
mod options {
    pub static FILE: &str = "file";
}

pub fn uu_app() -> clap::Command {
    let cmd = Command::new("cat");
    std::process::abort();
    cmd.arg(Arg::new(options::FILE))
}
"#,
            ),
            ("src/uu/cat/locales/en-US.ftl", ""),
        ]);

        let err = run(&uutils, "cat", "poc").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("found 3 statements"), "got: {msg}");
    }
}
