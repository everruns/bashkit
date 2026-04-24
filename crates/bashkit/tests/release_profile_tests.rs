// Regression tests for the workspace release profile.
// The interpreter wraps builtins in `catch_unwind`, which requires the
// release profile to unwind panics. `panic = "abort"` silently disables
// that containment and reintroduces the #1401 DoS regression.

use std::path::PathBuf;

fn workspace_cargo_toml() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("Cargo.toml")
}

fn release_profile_section(content: &str) -> &str {
    let after_header = content
        .split("[profile.release]")
        .nth(1)
        .expect("[profile.release] section must exist in workspace Cargo.toml");
    after_header.split("\n[").next().unwrap_or(after_header)
}

/// Regression for #1401: keep `panic = "unwind"` in the release profile.
#[test]
fn release_profile_keeps_panic_unwind() {
    let toml = workspace_cargo_toml();
    let content =
        std::fs::read_to_string(&toml).unwrap_or_else(|e| panic!("read {}: {e}", toml.display()));
    let section = release_profile_section(&content);

    assert!(
        section.contains("panic = \"unwind\""),
        "release profile must set `panic = \"unwind\"`; section was:\n{section}"
    );
    assert!(
        !section.contains("panic = \"abort\""),
        "release profile must not set `panic = \"abort\"`; section was:\n{section}"
    );
}
