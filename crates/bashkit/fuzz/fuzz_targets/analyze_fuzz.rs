//! Fuzz target for `Bash::analyze` (static script analysis)
//!
//! Hosts call `analyze()` on untrusted, model-generated scripts *before*
//! deciding whether to run them, so it must never panic or fabricate a command
//! name from plain source text. This target checks:
//! - Analysis crashes/panics on arbitrary input
//! - Stack overflows from deeply nested substitutions
//! - Reported plain names that do not appear in the script
//!
//! Run with: cargo +nightly fuzz run analyze_fuzz -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8 (bash scripts are text)
    if let Ok(input) = std::str::from_utf8(data) {
        // Limit input size to prevent OOM (threat model V1)
        if input.len() > 1_000_000 {
            return;
        }

        // Analysis must never panic; unparseable input is an error, not a
        // panic and not an empty analysis.
        let Ok(analysis) = bashkit::analysis::analyze(input) else {
            return;
        };

        // Quote/escape removal may join spans, but cannot insert or reorder
        // characters. ANSI-C $'...' is the exception because it decodes
        // escapes; deterministic tests cover malformed normalization syntax.
        if !input.contains("$'") {
            for command in &analysis.commands {
                if let Some(name) = command.name.as_deref() {
                    let mut source = input.chars();
                    assert!(
                        name.chars().all(|wanted| source.any(|ch| ch == wanted)),
                        "analysis inserted or reordered command-name characters"
                    );
                }
            }
        }

        // Budget invariant: recorded nodes never exceed the cap, and hitting
        // the cap must set `truncated` (which makes the script opaque).
        let nodes = analysis.commands.len() + analysis.redirects.len();
        assert!(nodes <= bashkit::analysis::MAX_ANALYSIS_NODES);
        if nodes == bashkit::analysis::MAX_ANALYSIS_NODES {
            assert!(analysis.truncated);
            assert!(analysis.is_opaque());
        }
    }
});
