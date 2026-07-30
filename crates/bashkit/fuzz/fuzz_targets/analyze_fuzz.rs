//! Fuzz target for `Bash::analyze` (static script analysis)
//!
//! Hosts call `analyze()` on untrusted, model-generated scripts *before*
//! deciding whether to run them, so it must never panic and must never
//! fabricate a command name that is not in the source. This target checks:
//! - Analysis crashes/panics on arbitrary input
//! - Stack overflows from deeply nested substitutions
//! - Reported literal names that do not appear in the script
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

        // A statically known name is quoted verbatim from the script, so it
        // must be present in the source text. A name conjured from nowhere
        // would let a host allowlist something the script never mentioned.
        for command in &analysis.commands {
            if let Some(name) = command.name.as_deref() {
                assert!(
                    input.contains(name),
                    "analysis reported a command name absent from the source"
                );
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
