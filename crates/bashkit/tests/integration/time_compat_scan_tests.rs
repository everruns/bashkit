// Regression guard for wasm32-unknown-unknown support.
// std::time::{Instant, SystemTime} panic at runtime on that target ("time not
// implemented on this platform"), so all wall-clock reads in the bashkit crate
// must go through crate::time_compat, which swaps in web-time on wasm32.
// This scan keeps new std::time::{Instant, SystemTime, UNIX_EPOCH} usage from
// creeping back in; the CI `cargo check --target wasm32-unknown-unknown` job
// catches breakage this lexical scan can't see (e.g. dependency regressions).
//
// Escape hatch: end the line with `// std-time-ok: <reason>` for code that is
// provably never compiled on wasm32 and has a real reason to bypass the shim.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The shim itself — `src/time_compat/` — is what everything else goes through.
fn is_time_compat(file: &Path) -> bool {
    file.components().any(|c| c.as_os_str() == "time_compat")
}

/// Everything except the shim itself must use `crate::time_compat` for
/// `Instant` / `SystemTime` / `UNIX_EPOCH`.
#[test]
fn std_time_clock_types_only_used_via_time_compat() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("time_compat/mod.rs")),
        "expected src/time_compat/mod.rs to exist"
    );

    let mut violations = Vec::new();
    for file in &files {
        if is_time_compat(file) {
            continue;
        }
        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        for (idx, line) in content.lines().enumerate() {
            let code = line.trim_start();
            // Doc comments may show std::time in public-API examples; doctests
            // compile against the public API on native, where that is correct.
            if code.starts_with("//") {
                continue;
            }
            if code.contains("// std-time-ok:") {
                continue;
            }
            let clock_types = ["Instant", "SystemTime", "UNIX_EPOCH"];
            let qualified = clock_types
                .iter()
                .any(|t| code.contains(&format!("std::time::{t}")));
            // Braced imports like `use std::time::{Duration, Instant};`.
            let braced = code
                .split("std::time::{")
                .nth(1)
                .and_then(|rest| rest.split('}').next())
                .is_some_and(|list| clock_types.iter().any(|t| list.contains(t)));
            if qualified || braced {
                violations.push(format!("{}:{}: {}", file.display(), idx + 1, code));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "std::time::{{Instant, SystemTime, UNIX_EPOCH}} panic on \
         wasm32-unknown-unknown; use crate::time_compat instead (or mark the \
         line `// std-time-ok: <reason>` if it can never be compiled for wasm32):\n{}",
        violations.join("\n")
    );
}

/// `chrono::Utc::now()` reads the clock through `std::time::SystemTime` (which
/// panics on wasm32-unknown-unknown) or, with chrono's `wasmbind` feature,
/// through the JS `Date` (which a non-JS wasm host cannot resolve). bashkit
/// builds chrono without `wasmbind`, so the clock must come from
/// `time_compat::now_utc()`.
#[test]
fn chrono_clock_only_read_via_time_compat() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);

    let mut violations = Vec::new();
    for file in &files {
        if is_time_compat(file) {
            continue;
        }
        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        for (idx, line) in content.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || code.contains("// std-time-ok:") {
                continue;
            }
            if code.contains("Utc::now()") || code.contains("Local::now()") {
                violations.push(format!("{}:{}: {}", file.display(), idx + 1, code));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "chrono's own clock is unavailable on wasm32; use \
         crate::time_compat::now_utc() instead (or mark the line \
         `// std-time-ok: <reason>` if it can never be compiled for wasm32):\n{}",
        violations.join("\n")
    );
}
