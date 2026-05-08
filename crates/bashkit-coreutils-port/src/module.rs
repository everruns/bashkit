//! Module mode — vendor a uucore module into bashkit.
//!
//! Algorithm:
//! 1. Load the manifest and look up the requested module entry.
//! 2. Walk every `.rs` file under the entry's `source` (single file
//!    or directory, depth-recursive).
//! 3. For each file, parse with syn and walk top-level `use` items:
//!    - `use fluent::*;` (or any `fluent::...`) → hard error: the
//!      module is not safely vendorable without code changes.
//!    - `use uucore::translate;` / `translate::*` → same hard error
//!      class (Fluent is the i18n boundary).
//!    - any other internal path (`uucore::`, `crate::`) must match a
//!      manifest substitution prefix. Unmatched paths abort the port
//!      so unexpected internal references surface explicitly.
//!    - matched `error` actions abort with a policy-rejection message.
//!    - matched `replace_with` actions are rewritten in place (see
//!      [`apply_substitutions`]). Use-paths starting with the prefix
//!      have the matched segments swapped for `target`; if the leaf
//!      changes an `as <orig>` rename is added.
//!    - matched `inline` actions vendor the file at `inline_source`
//!      next to the module's output dir and rewrite the use-path to
//!      `super::<leaf>::…` so the vendored module compiles.
//! 4. If any `replace_with` or `inline` substitutions are in scope,
//!    the rewritten AST is emitted via `prettyplease::unparse` (use
//!    groups become individual `use` items as a side effect).
//!    Otherwise the source is written verbatim. A banner is prepended
//!    either way.
//! 5. After the primary tree, every `inline` substitution drives a
//!    second port pass on its `inline_source`, with the same enforce
//!    plus rewrite policy applied so transitive uucore references still
//!    surface explicitly.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use proc_macro2::Span;
use syn::{Ident, Item, ItemUse, UseTree};

use crate::manifest::{Action, Manifest, Module, Substitution};

pub fn run(
    uutils_dir: &Path,
    module_name: &str,
    rev: &str,
    manifest_path: &Path,
    out_base: &Path,
) -> Result<Vec<PathBuf>> {
    let manifest_text = std::fs::read_to_string(manifest_path).with_context(|| {
        format!(
            "read vendored manifest {} (override with $BASHKIT_VENDORED_TOML)",
            manifest_path.display()
        )
    })?;
    let manifest = Manifest::parse(&manifest_text)
        .with_context(|| format!("parse manifest {}", manifest_path.display()))?;
    let module = manifest.find(module_name).ok_or_else(|| {
        anyhow!(
            "module '{}' not declared in {} — add a [[modules]] stanza",
            module_name,
            manifest_path.display()
        )
    })?;

    let src_root = uutils_dir.join(&module.source);
    if !src_root.exists() {
        bail!(
            "manifest source path does not exist: {} (uutils dir: {})",
            src_root.display(),
            uutils_dir.display()
        );
    }
    let out_root = out_base.join(&module.out);

    let mut written = Vec::new();
    if src_root.is_file() {
        port_file(&src_root, &out_root, module, rev, &module.source)?;
        written.push(out_root);
    } else {
        port_dir(
            &src_root,
            &out_root,
            module,
            rev,
            &module.source,
            &mut written,
        )?;
    }

    // Inline-vendor any `action = "inline"` substitutions alongside the
    // module. The inlined files land next to the module's `out` dir so
    // rewritten paths can resolve them as siblings.
    port_inlined(uutils_dir, module, rev, out_base, &mut written)?;

    Ok(written)
}

fn port_dir(
    src_dir: &Path,
    out_dir: &Path,
    module: &Module,
    rev: &str,
    rel_root: &str,
    written: &mut Vec<PathBuf>,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let entries = std::fs::read_dir(src_dir)
        .with_context(|| format!("read source dir {}", src_dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            let sub_out = out_dir.join(&name);
            let sub_rel = format!("{rel_root}/{}", name.to_string_lossy());
            port_dir(&path, &sub_out, module, rev, &sub_rel, written)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let sub_out = out_dir.join(&name);
            let sub_rel = format!("{rel_root}/{}", name.to_string_lossy());
            port_file(&path, &sub_out, module, rev, &sub_rel)?;
            written.push(sub_out);
        }
    }
    Ok(())
}

fn port_file(src: &Path, out: &Path, module: &Module, rev: &str, rel_path: &str) -> Result<()> {
    let text =
        std::fs::read_to_string(src).with_context(|| format!("read source {}", src.display()))?;
    let mut parsed =
        syn::parse_file(&text).with_context(|| format!("parse {} as rust", src.display()))?;
    enforce_use_policy(&parsed, module, rel_path)?;

    let body_text = if needs_rewrite(module) {
        apply_substitutions(&mut parsed, module)?;
        prettyplease::unparse(&parsed)
    } else {
        text
    };

    let banner = banner(rev, &module.name, rel_path);
    let body = format!("{banner}{body_text}");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir {}", parent.display()))?;
    }
    std::fs::write(out, body).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

fn needs_rewrite(module: &Module) -> bool {
    module
        .substitutions
        .iter()
        .any(|s| matches!(s.action, Action::ReplaceWith | Action::Inline))
}

fn port_inlined(
    uutils_dir: &Path,
    module: &Module,
    rev: &str,
    out_base: &Path,
    written: &mut Vec<PathBuf>,
) -> Result<()> {
    for sub in &module.substitutions {
        if sub.action != Action::Inline {
            continue;
        }
        let inline_source = sub.inline_source.as_ref().ok_or_else(|| {
            anyhow!(
                "manifest substitution prefix '{}' has action 'inline' but no 'inline_source' field",
                sub.prefix
            )
        })?;
        let src = uutils_dir.join(inline_source);
        if !src.exists() {
            bail!(
                "inline_source path does not exist: {} (uutils dir: {})",
                src.display(),
                uutils_dir.display()
            );
        }
        let inline_target = inline_target_path(sub)?;
        let out = out_base.join(&inline_target);

        // Each inlined file gets the same enforce + rewrite treatment as
        // the primary module so transitive uucore references either
        // substitute or surface explicitly.
        port_file(&src, &out, module, rev, inline_source)?;
        written.push(out);
    }
    Ok(())
}

/// Where on disk the inlined file lands. By default, derive from the
/// substitution prefix's leaf segment (e.g. `crate::extendedbigdecimal`
/// → `extendedbigdecimal.rs`). Manifest stanzas may override the
/// derived path in the future via a new field; today we infer.
fn inline_target_path(sub: &Substitution) -> Result<String> {
    let leaf = sub
        .prefix
        .rsplit("::")
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "inline substitution prefix '{}' has no leaf segment",
                sub.prefix
            )
        })?;
    Ok(format!("{leaf}.rs"))
}

fn banner(rev: &str, module_name: &str, rel_path: &str) -> String {
    format!(
        "// GENERATED by bashkit-coreutils-port. DO NOT EDIT.\n\
         //\n\
         // Source: uutils/coreutils@{rev} {rel_path}\n\
         // Regenerate: cargo run -p bashkit-coreutils-port -- port-module <UUTILS_DIR> {module_name} <REV>\n\
         //\n\
         // Original uutils licensed MIT; see THIRD_PARTY_LICENSES.\n\n",
    )
}

/// Walk top-level `use` items, flatten use-trees into individual paths,
/// and enforce the manifest's substitution policy on every internal
/// reference. Returns Err with a human-readable message at the first
/// violation.
fn enforce_use_policy(file: &syn::File, module: &Module, rel_path: &str) -> Result<()> {
    let mut paths = Vec::new();
    for item in &file.items {
        if let Item::Use(u) = item {
            collect_paths(&u.tree, &mut Vec::new(), &mut paths);
        }
    }

    for path in &paths {
        // Fluent boundary: any direct fluent:: import, or uucore::translate
        // (Fluent is uucore's i18n surface). Hard error: vendoring i18n is
        // not safely doable without code changes.
        if path.first().map(String::as_str) == Some("fluent") {
            bail!(
                "unresolved import: '{}' in {}: module is not safely vendorable without code changes (Fluent runtime is not vendored)",
                path.join("::"),
                rel_path
            );
        }
        if path.starts_with(&["uucore".to_string(), "translate".to_string()])
            || path.starts_with(&["uucore".to_string(), "i18n".to_string()])
        {
            bail!(
                "unresolved import: '{}' in {}: i18n surface is not vendorable (translate!/Fluent require runtime infrastructure not present in bashkit)",
                path.join("::"),
                rel_path
            );
        }

        // External: pass through. Anything not rooted at uucore/crate/self/super
        // is assumed to be a published crate (std, bigdecimal, …).
        if !is_internal(path) {
            continue;
        }

        // Internal: must match a substitution.
        match find_match(path, &module.substitutions) {
            None => bail!(
                "unresolved import: '{}' in {}: declare a [[modules.substitutions]] stanza in vendored.toml",
                path.join("::"),
                rel_path
            ),
            Some(s) => match s.action {
                Action::Error => bail!(
                    "import '{}' in {} forbidden by manifest substitution rule (prefix '{}', action 'error')",
                    path.join("::"),
                    rel_path,
                    s.prefix
                ),
                Action::ReplaceWith => {
                    if s.target.is_none() {
                        bail!(
                            "manifest substitution prefix '{}' has action 'replace_with' but no 'target' field",
                            s.prefix
                        );
                    }
                }
                Action::Inline => {
                    if s.inline_source.is_none() {
                        bail!(
                            "manifest substitution prefix '{}' has action 'inline' but no 'inline_source' field",
                            s.prefix
                        );
                    }
                }
            },
        }
    }
    Ok(())
}

/// Apply `replace_with` and `inline` substitutions across all top-level
/// `use` items.
///
/// Strategy: flatten each use tree into its leaf paths (with optional
/// renames), apply matching substitutions, then re-emit one `use` item
/// per leaf. Use groups (`use a::{b, c}`) are flattened — semantically
/// equivalent, but easier to rewrite without losing the formatting that
/// was going to be re-pretty-printed anyway.
///
/// Substitution rules:
/// - `replace_with`: when a leaf's path starts with `s.prefix`, the
///   matched prefix is replaced with `s.target`. If the rewritten
///   path's final segment differs from the original, an `as` rename
///   preserves call-site references (e.g. `use crate::error::Error as
///   UError;`).
/// - `inline`: the inlined file lives next to the module's `out` dir,
///   so the path is rewritten to point at it via `super::<leaf>`. The
///   leaf identifier in the use is preserved.
fn apply_substitutions(file: &mut syn::File, module: &Module) -> Result<()> {
    let mut new_items: Vec<Item> = Vec::with_capacity(file.items.len());
    for item in file.items.drain(..) {
        match item {
            Item::Use(u) => {
                let mut leaves: Vec<UseLeaf> = Vec::new();
                collect_leaves(&u.tree, &mut Vec::new(), &mut leaves);
                if leaves.is_empty() {
                    new_items.push(Item::Use(u));
                    continue;
                }
                for leaf in leaves {
                    let rewritten = rewrite_leaf(leaf, &module.substitutions)?;
                    new_items.push(Item::Use(build_item_use(&u, rewritten)));
                }
            }
            other => new_items.push(other),
        }
    }
    file.items = new_items;
    Ok(())
}

#[derive(Clone, Debug)]
struct UseLeaf {
    /// Path segments excluding the final identifier (which becomes the
    /// imported name or the source for a glob).
    path: Vec<String>,
    /// Final segment: either an imported identifier or `*` for glob.
    /// `Glob` is represented as `path = full path` and `tail = Glob`.
    tail: LeafTail,
}

#[derive(Clone, Debug)]
enum LeafTail {
    /// `use a::b::c;` or `use a::b::c as d;` — `name` is the source
    /// segment (`c`), `alias` is `d` (or None if no rename).
    Name { name: String, alias: Option<String> },
    /// `use a::b::*;`
    Glob,
}

fn collect_leaves(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<UseLeaf>) {
    match tree {
        UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            collect_leaves(&p.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(n) => {
            out.push(UseLeaf {
                path: prefix.clone(),
                tail: LeafTail::Name {
                    name: n.ident.to_string(),
                    alias: None,
                },
            });
        }
        UseTree::Rename(r) => {
            out.push(UseLeaf {
                path: prefix.clone(),
                tail: LeafTail::Name {
                    name: r.ident.to_string(),
                    alias: Some(r.rename.to_string()),
                },
            });
        }
        UseTree::Glob(_) => {
            out.push(UseLeaf {
                path: prefix.clone(),
                tail: LeafTail::Glob,
            });
        }
        UseTree::Group(g) => {
            for t in &g.items {
                collect_leaves(t, prefix, out);
            }
        }
    }
}

fn rewrite_leaf(leaf: UseLeaf, subs: &[Substitution]) -> Result<UseLeaf> {
    // Build the full path representing this leaf's import target. For
    // `Name { name }` the full path is `path + [name]`; for `Glob`
    // it's just `path`.
    let mut full = leaf.path.clone();
    if let LeafTail::Name { ref name, .. } = leaf.tail {
        full.push(name.clone());
    }

    let Some(sub) = find_rewriting_match(&full, subs) else {
        return Ok(leaf);
    };

    let target_segs: Vec<String> = match sub.action {
        Action::ReplaceWith => {
            let target = sub
                .target
                .as_ref()
                .expect("validated in enforce_use_policy");
            let segs: Vec<String> = target.split("::").map(String::from).collect();
            if segs.is_empty() {
                bail!(
                    "manifest substitution prefix '{}' has empty target",
                    sub.prefix
                );
            }
            segs
        }
        Action::Inline => {
            // Inlined file is a sibling of the module out dir. Use
            // `super::<leaf>` to reach it from the vendored module's
            // submodules.
            let leaf_seg = sub
                .prefix
                .rsplit("::")
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("inline prefix '{}' has no leaf segment", sub.prefix))?;
            vec!["super".to_string(), leaf_seg.to_string()]
        }
        Action::Error => unreachable!("error action does not reach the rewriter"),
    };

    // Replace the matched prefix with the target. The unmatched suffix
    // is preserved.
    let prefix_len = sub.prefix.split("::").count();
    let suffix = &full[prefix_len..];
    let mut rewritten_full: Vec<String> = target_segs;
    rewritten_full.extend_from_slice(suffix);

    // Split rewritten_full back into (path, tail). For glob preservation,
    // we keep the original tail kind.
    match leaf.tail {
        LeafTail::Glob => Ok(UseLeaf {
            path: rewritten_full,
            tail: LeafTail::Glob,
        }),
        LeafTail::Name {
            name: orig_name,
            alias: orig_alias,
        } => {
            // Final segment of rewritten_full is the new imported ident.
            let new_name = rewritten_full
                .pop()
                .ok_or_else(|| anyhow!("rewritten path is empty for prefix '{}'", sub.prefix))?;

            // Preserve the original call-site name. If the user already
            // had `as alias`, keep it. Otherwise, if rewriting changed
            // the last segment, alias to the original name.
            let alias = match orig_alias {
                Some(a) => Some(a),
                None if new_name != orig_name => Some(orig_name),
                None => None,
            };

            Ok(UseLeaf {
                path: rewritten_full,
                tail: LeafTail::Name {
                    name: new_name,
                    alias,
                },
            })
        }
    }
}

fn find_rewriting_match<'a>(path: &[String], subs: &'a [Substitution]) -> Option<&'a Substitution> {
    subs.iter()
        .filter(|s| matches!(s.action, Action::ReplaceWith | Action::Inline))
        .find(|s| {
            let segs: Vec<&str> = s.prefix.split("::").collect();
            path.len() >= segs.len() && path.iter().zip(&segs).all(|(a, b)| a == b)
        })
}

fn build_item_use(template: &ItemUse, leaf: UseLeaf) -> ItemUse {
    let tree = build_use_tree(&leaf);
    ItemUse {
        attrs: template.attrs.clone(),
        vis: template.vis.clone(),
        use_token: template.use_token,
        leading_colon: template.leading_colon,
        tree,
        semi_token: template.semi_token,
    }
}

fn build_use_tree(leaf: &UseLeaf) -> UseTree {
    let inner = match &leaf.tail {
        LeafTail::Name { name, alias } => {
            let ident = Ident::new(name, Span::call_site());
            match alias {
                Some(rename) => UseTree::Rename(syn::UseRename {
                    ident,
                    as_token: syn::Token![as](Span::call_site()),
                    rename: Ident::new(rename, Span::call_site()),
                }),
                None => UseTree::Name(syn::UseName { ident }),
            }
        }
        LeafTail::Glob => UseTree::Glob(syn::UseGlob {
            star_token: syn::Token![*](Span::call_site()),
        }),
    };

    leaf.path.iter().rev().fold(inner, |acc, seg| {
        UseTree::Path(syn::UsePath {
            ident: Ident::new(seg, Span::call_site()),
            colon2_token: syn::Token![::](Span::call_site()),
            tree: Box::new(acc),
        })
    })
}

fn is_internal(path: &[String]) -> bool {
    matches!(
        path.first().map(String::as_str),
        Some("uucore" | "crate" | "self" | "super")
    )
}

fn find_match<'a>(path: &[String], subs: &'a [Substitution]) -> Option<&'a Substitution> {
    subs.iter().find(|s| {
        let segs: Vec<&str> = s.prefix.split("::").collect();
        path.len() >= segs.len() && path.iter().zip(&segs).all(|(a, b)| a == b)
    })
}

fn collect_paths(tree: &UseTree, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match tree {
        UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            collect_paths(&p.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(n) => {
            let mut path = prefix.clone();
            path.push(n.ident.to_string());
            out.push(path);
        }
        UseTree::Rename(r) => {
            let mut path = prefix.clone();
            path.push(r.ident.to_string());
            out.push(path);
        }
        UseTree::Glob(_) => {
            let mut path = prefix.clone();
            path.push("*".into());
            out.push(path);
        }
        UseTree::Group(g) => {
            for t in &g.items {
                collect_paths(t, prefix, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Build a minimal vendored.toml + uutils tree under a tempdir and
    /// return (uutils_dir, manifest_path, out_base).
    fn fixture(manifest: &str, files: &[(&str, &str)]) -> (TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let uutils = tmp.path().join("uutils");
        let manifest_path = tmp.path().join("vendored.toml");
        let out = tmp.path().join("out");
        for (rel, content) in files {
            let path = uutils.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        fs::create_dir_all(&out).unwrap();
        fs::write(&manifest_path, manifest).unwrap();
        (tmp, uutils, manifest_path, out)
    }

    #[test]
    fn happy_path_external_imports_only() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"
"#,
            &[(
                "lib/demo.rs",
                "use std::collections::HashMap;\nuse bigdecimal::BigDecimal;\npub fn x() {}\n",
            )],
        );
        let written = run(&uutils, "demo", "abc123", &manifest, &out).unwrap();
        assert_eq!(written.len(), 1);
        let body = fs::read_to_string(&written[0]).unwrap();
        assert!(body.starts_with("// GENERATED by bashkit-coreutils-port"));
        assert!(body.contains("uutils/coreutils@abc123"));
        assert!(body.contains("use std::collections::HashMap;"));
        assert!(body.contains("pub fn x() {}"));
    }

    #[test]
    fn fluent_import_hard_errors() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"
"#,
            &[("lib/demo.rs", "use fluent::FluentBundle;\n")],
        );
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("not safely vendorable"), "got: {msg}");
    }

    #[test]
    fn uucore_translate_hard_errors() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"
"#,
            &[("lib/demo.rs", "use uucore::translate;\n")],
        );
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("i18n surface"), "got: {msg}");
    }

    #[test]
    fn unresolved_uucore_import_errors() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"
"#,
            &[("lib/demo.rs", "use uucore::error::UError;\n")],
        );
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unresolved import"), "got: {msg}");
        assert!(msg.contains("vendored.toml"), "got: {msg}");
    }

    #[test]
    fn error_action_aborts_port() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "error"
"#,
            &[("lib/demo.rs", "use uucore::error::UError;\n")],
        );
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("forbidden by manifest"), "got: {msg}");
    }

    #[test]
    fn replace_with_action_rewrites_use_path() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "replace_with"
target = "crate::error::Error"
"#,
            &[("lib/demo.rs", "use uucore::error::UError;\n")],
        );
        let written = run(&uutils, "demo", "x", &manifest, &out).unwrap();
        assert_eq!(written.len(), 1);
        let body = fs::read_to_string(&written[0]).unwrap();
        assert!(
            body.contains("use crate::error::Error as UError;"),
            "got: {body}"
        );
        assert!(!body.contains("uucore::error::UError"), "got: {body}");
    }

    #[test]
    fn replace_with_preserves_matching_leaf_without_alias() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"

[[modules.substitutions]]
prefix = "uucore::extendedbigdecimal"
action = "replace_with"
target = "crate::extendedbigdecimal"
"#,
            &[(
                "lib/demo.rs",
                "use uucore::extendedbigdecimal::ExtendedBigDecimal;\n",
            )],
        );
        let written = run(&uutils, "demo", "x", &manifest, &out).unwrap();
        let body = fs::read_to_string(&written[0]).unwrap();
        assert!(
            body.contains("use crate::extendedbigdecimal::ExtendedBigDecimal;"),
            "got: {body}"
        );
        assert!(!body.contains(" as "), "no alias needed; got: {body}");
    }

    #[test]
    fn replace_with_flattens_use_groups() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "replace_with"
target = "crate::error::Error"

[[modules.substitutions]]
prefix = "uucore::extendedbigdecimal::ExtendedBigDecimal"
action = "replace_with"
target = "crate::extendedbigdecimal::ExtendedBigDecimal"
"#,
            &[(
                "lib/demo.rs",
                "use uucore::{error::UError, extendedbigdecimal::ExtendedBigDecimal};\n",
            )],
        );
        let written = run(&uutils, "demo", "x", &manifest, &out).unwrap();
        let body = fs::read_to_string(&written[0]).unwrap();
        assert!(
            body.contains("use crate::error::Error as UError;"),
            "got: {body}"
        );
        assert!(
            body.contains("use crate::extendedbigdecimal::ExtendedBigDecimal;"),
            "got: {body}"
        );
    }

    #[test]
    fn replace_with_preserves_existing_alias() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "replace_with"
target = "crate::error::Error"
"#,
            &[("lib/demo.rs", "use uucore::error::UError as MyErr;\n")],
        );
        let written = run(&uutils, "demo", "x", &manifest, &out).unwrap();
        let body = fs::read_to_string(&written[0]).unwrap();
        assert!(
            body.contains("use crate::error::Error as MyErr;"),
            "got: {body}"
        );
    }

    #[test]
    fn replace_with_missing_target_fails() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "replace_with"
"#,
            &[("lib/demo.rs", "use uucore::error::UError;\n")],
        );
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no 'target'"), "got: {msg}");
    }

    #[test]
    fn module_not_in_manifest() {
        let (_tmp, uutils, manifest, out) = fixture("", &[]);
        let err = run(&uutils, "absent", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("not declared"), "got: {msg}");
    }

    #[test]
    fn directory_source_walks_recursively() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo"
out = "demo"
"#,
            &[
                ("lib/demo/mod.rs", "pub mod inner;\npub fn a() {}\n"),
                ("lib/demo/inner.rs", "use std::io;\npub fn b() {}\n"),
            ],
        );
        let written = run(&uutils, "demo", "v1", &manifest, &out).unwrap();
        assert_eq!(written.len(), 2, "got: {written:?}");
        for p in &written {
            let body = fs::read_to_string(p).unwrap();
            assert!(body.starts_with("// GENERATED by bashkit-coreutils-port"));
        }
    }

    #[test]
    fn nested_use_groups_are_flattened() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo.rs"
"#,
            &[("lib/demo.rs", "use uucore::{error::UError, format::sci};\n")],
        );
        // The first internal path in the group should surface as
        // unresolved — verifies group flattening.
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unresolved import"), "got: {msg}");
    }

    #[test]
    fn inline_action_vendors_source_file_alongside() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo"

[[modules.substitutions]]
prefix = "uucore::extendedbigdecimal"
action = "inline"
inline_source = "lib/extendedbigdecimal.rs"
"#,
            &[
                (
                    "lib/demo.rs",
                    "use uucore::extendedbigdecimal::ExtendedBigDecimal;\n",
                ),
                (
                    "lib/extendedbigdecimal.rs",
                    "use std::fmt::Display;\npub struct ExtendedBigDecimal;\n",
                ),
            ],
        );
        let written = run(&uutils, "demo", "x", &manifest, &out).unwrap();
        assert_eq!(written.len(), 2, "got: {written:?}");

        // Module body uses super::extendedbigdecimal to reach the
        // sibling-vendored file.
        let module_body = fs::read_to_string(&written[0]).unwrap();
        assert!(
            module_body.contains("use super::extendedbigdecimal::ExtendedBigDecimal;"),
            "got: {module_body}"
        );

        // Inlined file is vendored next to the module with its own banner.
        let inlined_body = fs::read_to_string(&written[1]).unwrap();
        assert!(
            inlined_body.starts_with("// GENERATED by bashkit-coreutils-port"),
            "got: {inlined_body}"
        );
        assert!(
            inlined_body.contains("pub struct ExtendedBigDecimal;"),
            "got: {inlined_body}"
        );
    }

    #[test]
    fn inline_missing_inline_source_field_fails() {
        let (_tmp, uutils, manifest, out) = fixture(
            r#"
[[modules]]
name = "demo"
source = "lib/demo.rs"
out = "demo"

[[modules.substitutions]]
prefix = "uucore::extendedbigdecimal"
action = "inline"
"#,
            &[(
                "lib/demo.rs",
                "use uucore::extendedbigdecimal::ExtendedBigDecimal;\n",
            )],
        );
        let err = run(&uutils, "demo", "x", &manifest, &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("inline_source"), "got: {msg}");
    }
}
