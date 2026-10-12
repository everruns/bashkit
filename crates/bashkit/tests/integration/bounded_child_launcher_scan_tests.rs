// Regression guard for supervised-child test harnesses under sanitizers.
//
// A bounded-child launcher caps CPU (`ulimit -t`) and stack (`ulimit -s`) and
// leaves memory to the parent-side peak-RSS watchdog. An address-space cap
// (`ulimit -v`, RLIMIT_AS) must never appear: AddressSanitizer reserves ~14 TiB
// of virtual address space for its shadow map at startup, so an instrumented
// child dies in `ReserveShadowMemoryRange` before running a line of test code.
// The child then fails on the launcher's cap rather than the bound under test,
// which is how nightly run 257 went red on `awk_field_depth_bounded` while
// required CI, which is not instrumented, stayed green on the same commit.
//
// See knowledge/operations/testing.md, "Bounded-child launchers cap CPU and
// stack, never address space".
//
// Escape hatch: end the line with `// rlimit-as-ok: <reason>` for a launcher
// that is provably never built with a sanitizer and has a real reason to cap
// address space.

use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate must live under repo/crates")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            // `target` directories hold build output, not sources to police.
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// No test launcher caps the child's address space.
#[test]
fn bounded_child_launchers_never_cap_address_space() {
    let mut files = Vec::new();
    rust_files(&crates_dir(), &mut files);
    // Fail closed: a scan that found nothing proves nothing.
    assert!(
        files
            .iter()
            .any(|f| f.ends_with("integration/awk_field_depth_tests.rs")),
        "scan did not reach the known bounded-child harnesses"
    );

    let mut offenders = Vec::new();
    for file in &files {
        if file.ends_with("integration/bounded_child_launcher_scan_tests.rs") {
            continue;
        }
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        for (number, line) in source.lines().enumerate() {
            let code = line.trim();
            // Prose explaining the rule names the caps it bans, so comment
            // lines are described, not enforced.
            if code.starts_with("//") || code.contains("// rlimit-as-ok:") {
                continue;
            }
            // Collapse runs of whitespace so `ulimit   -v` cannot slip past.
            let flat = code.split_whitespace().collect::<Vec<_>>().join(" ");
            // `-d`/`RLIMIT_DATA` cap the data segment and break ASAN the same
            // way; the `RLIMIT_*` forms catch a direct `setrlimit` in Rust,
            // which the shell spellings alone would miss.
            let banned = ["ulimit -v", "ulimit -d", "RLIMIT_AS", "RLIMIT_DATA"];
            if banned.iter().any(|pattern| flat.contains(pattern)) {
                offenders.push(format!("{}:{}: {code}", file.display(), number + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "address-space caps break sanitizer-instrumented children; cap RSS from \
         the parent instead (knowledge/operations/testing.md):\n{}",
        offenders.join("\n")
    );
}
