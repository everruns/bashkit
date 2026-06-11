//! Dump the canonical builtin inventory as JSON.
//!
//! Source generator for `specs/status/builtins.json` (the data behind the
//! site's builtins page). Run via `just regen-builtins`, which enables every
//! builtin-affecting feature so the inventory is complete; the
//! `builtins-drift` workflow regenerates and fails on diff.
//!
//! Attribution: builder-registered opt-ins (python, typescript, sqlite) are
//! discovered by diffing `builtin_names()` against a default `Bash`;
//! cfg-registered families are tagged from the table below, which must stay
//! in sync with the `#[cfg(feature = ...)]` blocks in
//! `interpreter::with_config`.

use bashkit::Bash;
use std::collections::BTreeMap;

/// Families registered by compile feature alone (present in a default-built
/// `Bash` whenever the feature is on).
const CFG_REGISTERED: &[(&str, &[&str])] = &[
    ("jq", &["jq"]),
    ("git", &["git"]),
    ("ssh", &["ssh", "scp", "sftp"]),
];

fn main() {
    let base = Bash::new().builtin_names();
    let mut feature_of: BTreeMap<String, String> = BTreeMap::new();

    for (feature, names) in CFG_REGISTERED {
        for name in *names {
            if base.iter().any(|n| n == name) {
                feature_of.insert((*name).to_string(), (*feature).to_string());
            }
        }
    }

    let mut tag_builder_optin = |names: Vec<String>, feature: &str| {
        for name in names {
            if !base.contains(&name) {
                feature_of.insert(name, feature.to_string());
            }
        }
    };

    #[cfg(feature = "python")]
    tag_builder_optin(Bash::builder().python().build().builtin_names(), "python");
    #[cfg(feature = "typescript")]
    tag_builder_optin(
        Bash::builder().typescript().build().builtin_names(),
        "typescript",
    );
    #[cfg(feature = "sqlite")]
    tag_builder_optin(Bash::builder().sqlite().build().builtin_names(), "sqlite");
    #[cfg(not(any(feature = "python", feature = "typescript", feature = "sqlite")))]
    let _ = &mut tag_builder_optin;

    let mut all: Vec<String> = base;
    all.extend(feature_of.keys().cloned());
    all.sort();
    all.dedup();

    let builtins: Vec<serde_json::Value> = all
        .iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "feature": feature_of.get(name),
            })
        })
        .collect();

    let doc = serde_json::json!({
        "_generated": "just regen-builtins — do not edit by hand",
        "count": builtins.len(),
        "builtins": builtins,
    });
    println!("{}", serde_json::to_string_pretty(&doc).expect("serialize"));
}
