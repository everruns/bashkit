//! `vendored.toml` schema — declarative inventory of uucore modules
//! vendored into bashkit by `bashkit-coreutils-port port-module`, plus
//! per-import substitution policy.
//!
//! Single-source-of-truth: the drift workflow iterates this manifest
//! to re-port every entry against uutils HEAD. Adding a new vendored
//! module is one TOML stanza.
//!
//! Schema:
//!
//! ```toml
//! # Top-level: list of modules. Each entry is one porting target.
//! [[modules]]
//! name = "format"                              # unique id, used on CLI
//! source = "src/uucore/src/lib/features/format" # under <UUTILS_DIR>; file or dir
//! out = "format"                               # under generated/; file or dir
//!
//! # `substitutions` declare how to handle uucore-internal `use` imports
//! # encountered while porting. Any uucore-internal `use` path that does
//! # not match a substitution prefix aborts the port — silent emission of
//! # broken imports is rejected.
//! [[modules.substitutions]]
//! prefix = "uucore::error::UError"  # leading-segment match against the use path
//! action = "error"                  # forbid the import outright
//!
//! [[modules.substitutions]]
//! prefix = "uucore::extendedbigdecimal"
//! action = "inline"                 # vendor the source defining this type too
//! inline_source = "src/uucore/src/lib/features/extendedbigdecimal.rs"
//!
//! [[modules.substitutions]]
//! prefix = "uucore::error::UError"
//! action = "replace_with"           # rewrite the import to a bashkit-side type
//! target = "crate::error::Error"
//! ```
//!
//! Action support, current implementation:
//!
//! - `error` — fully implemented (port aborts when matched).
//! - `inline`, `replace_with` — accepted in the schema, but require
//!   the future `syn`-based import rewriter; the tool errors at
//!   runtime if a module's substitution declares them. Manifest-side
//!   declarations stay forward-compatible: when the rewriter lands,
//!   the same manifest works without further changes.

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub modules: Vec<Module>,
}

#[derive(Debug, Deserialize)]
pub struct Module {
    pub name: String,
    pub source: String,
    pub out: String,
    #[serde(default)]
    pub substitutions: Vec<Substitution>,
}

// `target` and `inline_source` are read by the future syn-based import
// rewriter (#1534). Kept on the struct now so the manifest schema is
// stable across the rewriter landing — otherwise existing manifest
// stanzas would have to change shape on first use.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Substitution {
    pub prefix: String,
    pub action: Action,
    /// `replace_with`: replacement prefix.
    #[serde(default)]
    pub target: Option<String>,
    /// `inline`: source path (relative to uutils dir) of the file
    /// that defines the substituted type.
    #[serde(default)]
    pub inline_source: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Inline,
    ReplaceWith,
    Error,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Inline => "inline",
            Action::ReplaceWith => "replace_with",
            Action::Error => "error",
        }
    }
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn find(&self, name: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal() {
        let m = Manifest::parse("").unwrap();
        assert!(m.modules.is_empty());
    }

    #[test]
    fn parses_full_entry() {
        let toml = r#"
[[modules]]
name = "format"
source = "src/uucore/src/lib/features/format"
out = "format"

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "error"

[[modules.substitutions]]
prefix = "uucore::extendedbigdecimal"
action = "inline"
inline_source = "src/uucore/src/lib/features/extendedbigdecimal.rs"

[[modules.substitutions]]
prefix = "uucore::error::SomeOther"
action = "replace_with"
target = "crate::error::Other"
"#;
        let m = Manifest::parse(toml).unwrap();
        assert_eq!(m.modules.len(), 1);
        let entry = m.find("format").unwrap();
        assert_eq!(entry.source, "src/uucore/src/lib/features/format");
        assert_eq!(entry.substitutions.len(), 3);
        assert_eq!(entry.substitutions[0].action, Action::Error);
        assert_eq!(entry.substitutions[1].action, Action::Inline);
        assert_eq!(entry.substitutions[2].action, Action::ReplaceWith);
    }

    #[test]
    fn rejects_unknown_action() {
        let toml = r#"
[[modules]]
name = "x"
source = "x"
out = "x"

[[modules.substitutions]]
prefix = "uucore::x"
action = "nope"
"#;
        assert!(Manifest::parse(toml).is_err());
    }
}
