//! Port a uutils/coreutils utility's `uu_app()` clap definition into a
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
//! 4. Emit a generated file containing only:
//!    - `mod options { pub static ... }` (verbatim from source)
//!    - `pub fn <util>_command() -> clap::Command` (rewritten `uu_app`)
//!    - `fn format_usage(s: &str) -> String` (vendored shim)
//!
//! Usage:
//!
//! ```text
//! bashkit-coreutils-port <UUTILS_DIR> <UTIL> [<UUTILS_REV>]
//! ```
//!
//! Output: prettyprinted Rust to stdout (caller redirects to file).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use proc_macro2::TokenStream;
use quote::quote;
use syn::visit_mut::{self, VisitMut};
use syn::{Expr, ExprCall, ExprMacro, Item, ItemFn, ItemMod, LitStr, parse_quote};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (uutils_dir, util, rev): (PathBuf, String, String) = match args.as_slice() {
        [_, dir, util] => (PathBuf::from(dir), util.clone(), "unknown".into()),
        [_, dir, util, rev] => (PathBuf::from(dir), util.clone(), rev.clone()),
        _ => {
            eprintln!("usage: bashkit-coreutils-port <UUTILS_DIR> <UTIL> [<UUTILS_REV>]");
            return ExitCode::from(2);
        }
    };
    match run(&uutils_dir, &util, &rev) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(uutils_dir: &Path, util: &str, rev: &str) -> Result<String> {
    let src_path = uutils_dir
        .join("src/uu")
        .join(util)
        .join("src")
        .join(format!("{util}.rs"));
    let ftl_path = uutils_dir
        .join("src/uu")
        .join(util)
        .join("locales/en-US.ftl");

    let src = std::fs::read_to_string(&src_path)
        .with_context(|| format!("read uutils source {}", src_path.display()))?;
    let ftl = std::fs::read_to_string(&ftl_path)
        .with_context(|| format!("read uutils ftl {}", ftl_path.display()))?;

    let translations = parse_ftl(&ftl);
    let file = syn::parse_file(&src).context("parse uutils source as rust")?;

    // Two option-key declaration styles in the wild:
    // 1. `mod options { pub static FOO: &str = "foo"; ... }` (cat, tac,
    //    truncate, stat, shuf). uu_app() refers to keys via
    //    `options::FOO`.
    // 2. Module-level `const OPT_FOO: &str = "foo";` / `static OPT_FOO`
    //    (mktemp, realpath, readlink, od). uu_app() refers to keys by
    //    bare name (`OPT_FOO`).
    //
    // Collect both: the optional `options` mod (if present) and any
    // bare-name `OPT_*` / `ARG_*` constants we should also emit so
    // uu_app's bare-name references resolve.
    let options_mod = find_mod(&file, "options").cloned();
    let bare_name_consts = collect_option_constants(&file);
    if options_mod.is_none() && bare_name_consts.is_empty() {
        bail!(
            "could not find `mod options` or any module-level `OPT_*`/`ARG_*` \
             constants in {}",
            src_path.display()
        );
    }
    let mut uu_app = find_fn(&file, "uu_app")
        .ok_or_else(|| anyhow!("could not find `fn uu_app` in {}", src_path.display()))?
        .clone();

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

    // Some uu_app() definitions reference free helper functions from the
    // same source file (e.g. shuf's `parse_range` used as a clap
    // value_parser). Inline those helpers so the generated file
    // compiles standalone, running them through the same rewriter so
    // their translate!() calls resolve too.
    let helpers = collect_referenced_helpers(&uu_app, &file)
        .into_iter()
        .map(|mut helper| {
            rw.visit_item_fn_mut(&mut helper);
            helper
        })
        .collect::<Vec<_>>();
    if !rw.unresolved.is_empty() {
        bail!(
            "unresolved translate!() keys in inlined helpers: {:?}",
            rw.unresolved
        );
    }

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
    let options_mod_tokens: TokenStream = match options_mod {
        Some(m) => quote!(#m),
        None => quote!(),
    };
    // Bare-name constants for utils that don't wrap them in a mod.
    let const_tokens: Vec<TokenStream> = bare_name_consts
        .into_iter()
        .map(|c| quote::quote!(#c))
        .collect();

    let body: TokenStream = quote! {
        #![allow(unused_imports, dead_code)]

        // Always import the broader clap+std surface a few utils need.
        // `#![allow(unused_imports)]` above silences warnings for
        // utilities that don't reach for them. Add to this list when a
        // newly-ported util needs another std type that the inlined
        // helpers reference (e.g. shuf's `parse_range` returns
        // `RangeInclusive<u64>`).
        use clap::{Arg, ArgAction, Command, builder::ValueParser};
        use std::ffi::OsString;
        use std::ops::RangeInclusive;
        use std::str::FromStr;

        #options_mod_tokens
        #(#const_tokens)*

        /// Vendored stand-in for `uucore::format_usage`.
        ///
        /// Upstream wraps the usage line with stylized "Usage:" prefix logic
        /// driven by uucore's locale stack. For our purposes the raw string
        /// is enough; clap's `override_usage` accepts the literal as-is.
        fn format_usage(s: &str) -> String {
            s.to_string()
        }

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

/// Walk `uu_app()`'s body looking for plain identifier references
/// (single-segment paths) and return any matching free `fn` defined at
/// the top level of the source file. Skips items already provided by
/// the generated preamble (e.g. `format_usage`).
///
/// One-level only — if a copied helper itself references another local
/// helper, we surface that as a compile error rather than recursively
/// inlining. The shuf use case (`parse_range` referencing only `std`
/// and `translate!()`) doesn't need recursion; broaden when a real
/// case demands it.
fn collect_referenced_helpers(uu_app: &ItemFn, file: &syn::File) -> Vec<ItemFn> {
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

    file.items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(f) => {
                let name = f.sig.ident.to_string();
                if names.contains(&name) && !PROVIDED_BY_PREAMBLE.contains(&name.as_str()) {
                    Some(f.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

fn find_mod<'a>(file: &'a syn::File, name: &str) -> Option<&'a ItemMod> {
    file.items.iter().find_map(|it| match it {
        Item::Mod(m) if m.ident == name => Some(m),
        _ => None,
    })
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

fn find_fn<'a>(file: &'a syn::File, name: &str) -> Option<&'a ItemFn> {
    file.items.iter().find_map(|it| match it {
        Item::Fn(f) if f.sig.ident == name => Some(f),
        _ => None,
    })
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
