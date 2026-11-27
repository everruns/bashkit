//! Replays of the exact inputs that turned nightly fuzz runs red.
//!
//! Each case embeds the libFuzzer crash unit (base64 from the run log) and
//! rebuilds the script the way its fuzz target does, so the real builtin
//! runs and the real `fuzz_exec` invariants apply. libFuzzer itself is the
//! only part not exercised — these are regressions in what the harness
//! considers a leak, not in the fuzzer.
//!
//! Run with: cargo test --test fuzz_crash_regressions

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, ExecutionLimits};
use std::time::Duration;

/// Decode a base64 crash unit. Hand-rolled so the test needs no dev-dependency.
fn b64(s: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for ch in s.bytes().filter(|&b| b != b'=') {
        let v = ALPHABET
            .iter()
            .position(|&a| a == ch)
            .expect("base64 alphabet") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn bash(max_stdout: usize) -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .max_subst_depth(3)
                .max_stdout_bytes(max_stdout)
                .max_stderr_bytes(4096)
                .timeout(Duration::from_millis(200)),
        )
        .build()
}

fn sq(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// `envsubst_fuzz` run 304: the input carried the literal `Undefined::`,
/// envsubst echoed it, and the detector flagged its own dictionary entry.
#[tokio::test]
async fn envsubst_crash_6c5375e0_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("AEVSUgAKUwAKCgpVbmRlZmluZWQ6OgAKAAAAMDAyMDAA");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .max_subst_depth(3)
                .max_stdout_bytes(4096)
                .max_stderr_bytes(4096)
                .timeout(Duration::from_millis(200)),
        )
        .env("HOME", "/home/user")
        .env("PATH", "/usr/bin")
        .env("LANG", "en_US.UTF-8")
        .env("TESTVAR", "hello world")
        .build();
    for script in [
        format!("echo '{}' | envsubst", sq(input)),
        format!("echo '{}' | envsubst -v", sq(input)),
        format!("echo '{}' | envsubst '$HOME $PATH'", sq(input)),
    ] {
        fuzz_exec(&mut bash, &script, "envsubst_fuzz", &[]).await;
    }
}

/// `printf_fuzz` run 304: the `invalid number` diagnostic quoted
/// `builtin failed unexpectedly` straight out of the argument.
#[tokio::test]
async fn printf_crash_builtin_failed_unexpectedly_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("JSApJQoKCgoKCgogKWJ1aWx0aW4gZmFpbGVkIHVuZXhwZWN0ZWRseVQ=");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let (format_str, args_str) = match input.find('\n') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => (input, ""),
    };
    let args_joined = args_str
        .lines()
        .map(|a| format!("'{}'", sq(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut bash = bash(4096);
    for script in [
        format!("printf '{}' {}", sq(format_str), args_joined),
        format!("printf -v result '{}' {}", sq(format_str), args_joined),
        format!("printf '%d %o %x' {args_joined}"),
    ] {
        fuzz_exec(&mut bash, &script, "printf_fuzz", &[]).await;
    }
}

/// `csv_fuzz` run 304: the input carried `/.rustup/toolchains/`, which the
/// csv diagnostic echoed back.
#[tokio::test]
async fn csv_crash_rustup_toolchains_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("AEVSUgAnCl8vLnJ1c3R1cC90b29sY2hhaW5zLwAAAAAA");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let mut bash = bash(4096);
    for script in [
        format!("echo '{}' | csv headers", sq(input)),
        format!("echo '{}' | csv count", sq(input)),
        format!("echo '{}' | csv select 1", sq(input)),
        format!("echo '{}' | csv -d '\\t' headers", sq(input)),
    ] {
        fuzz_exec(&mut bash, &script, "csv_fuzz", &[]).await;
    }
}

/// `csv_fuzz`, Fuzz Testing run 45932089002: the input carried the host canary
/// with a `"` spliced into it (`..._D"O_NOT_LEAK`). `echo` passed that through
/// verbatim, then `csv headers` dropped the `"` as a CSV quote character and
/// printed the canary whole — so stdout held a token the script text never
/// spelled, and the TM-INF-013 assertion fired with no builtin having read
/// host env. libFuzzer harvests the canary from the detector's own `contains`,
/// so this is a forgery the harness armed; attribution now looks through the
/// characters a parser drops (`DROPPED_BY_PARSERS`).
#[tokio::test]
async fn csv_crash_quote_forged_canary_is_an_echo_not_a_leak() {
    fuzz_init();
    let data =
        b64("CgAKLwoACgAKCkJBU0hLSVRfRlVaWl9IT1NUX0NBTkFSWV80N2E4M2JjZl9EIk9fTk9UX0xFQUsFCgUKW1s=");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let mut bash = bash(4096);
    for script in [
        format!("echo '{}' | csv headers", sq(input)),
        format!("echo '{}' | csv count", sq(input)),
        format!("echo '{}' | csv select 1", sq(input)),
        format!("echo '{}' | csv -d '\\t' headers", sq(input)),
    ] {
        fuzz_exec(&mut bash, &script, "csv_fuzz", &[]).await;
    }
}

/// `printf_fuzz`, Fuzz Testing run 45932089002: `printf '%d %o %x'` cycles its
/// format over every operand, so this unit's 34 non-numeric operands each drew
/// an `invalid number` diagnostic — 1,283 bytes of bounded, legitimate stderr,
/// under the `max_stderr_bytes(4096)` the target configures but over the
/// harness's old 1 KiB cap. Not a flood: the harness was holding a
/// whole-execution budget to a single diagnostic's size.
#[tokio::test]
async fn printf_crash_per_operand_diagnostics_are_not_a_flood() {
    fuzz_init();
    let data = b64(
        "SwoKDgogCg4KIAoOCgoOCiAKDgoOCiAKDgolCg4KIAoOCiAKDgoOCiBACg4OCiAKDgolCg4KIAoOCiAKDgoOCiBACg4KIA==",
    );
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let (format_str, args_str) = match input.find('\n') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => (input, ""),
    };
    let args_joined = args_str
        .lines()
        .map(|a| format!("'{}'", sq(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut bash = bash(4096);
    for script in [
        format!("printf '{}' {}", sq(format_str), args_joined),
        format!("printf -v result '{}' {}", sq(format_str), args_joined),
        format!("printf '%d %o %x' {args_joined}"),
    ] {
        fuzz_exec(&mut bash, &script, "printf_fuzz", &[]).await;
    }
}

/// `tomlq_fuzz` run 304: `tomlq: path '<query>' not found` quotes the query
/// verbatim, and the query was `/.rustup/toolchains/…`.
#[tokio::test]
async fn tomlq_crash_rustup_toolchains_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("XQovLnJ1c3R1cC90b29sY2hhaW5zL9uoCg==");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let (toml_doc, query) = match input.find('\n') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => (input, "."),
    };
    let mut bash = bash(4096);
    for script in [
        format!("echo '{}' | tomlq '{}'", sq(toml_doc), sq(query)),
        format!("echo '{}' | tomlq", sq(toml_doc)),
    ] {
        fuzz_exec(&mut bash, &script, "tomlq_fuzz", &[]).await;
    }
}

/// `archive_fuzz` run 304: the input carried `Undefined::`. Tests 1 and 2
/// embed the bytes as `\xNN` escapes and cannot echo the shape at all; it was
/// the `gzip | gunzip` roundtrip, which inlines the text verbatim, that put
/// the input back on stderr.
#[tokio::test]
async fn archive_crash_undefined_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("AEVSUgAAVW5kZWZpbmVkOjoAAAAAAAB+Pw==");
    let hex: String = data.iter().map(|b| format!("\\x{b:02x}")).collect();
    let mut bash = bash(8192);
    for script in [
        format!("printf '{hex}' | gunzip"),
        format!("printf '{hex}' | tar -tf -"),
    ] {
        fuzz_exec(&mut bash, &script, "archive_fuzz", &[]).await;
    }
    let text = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let script3 = format!("echo '{}' | gzip | gunzip", sq(text));
    fuzz_exec(&mut bash, &script3, "archive_fuzz", &[]).await;
}

/// The minimized form of the `printf_fuzz` failure: libFuzzer reduces its
/// units, so the steady state for this class is an operand that is *only* the
/// banned shape. The diagnostic then frames it with template text on both
/// sides (`printf: ` / `: invalid number`), leaving no script text adjacent to
/// the occurrence. Attribution still excuses it; a rule keyed on neighbouring
/// script content would not, which is why `script_explains` is whole-script.
#[tokio::test]
async fn a_bare_banned_shape_framed_only_by_template_text_is_an_echo() {
    fuzz_init();
    let mut bash = bash(4096);
    for shape in ["Undefined::", "Tok::", "internal error:"] {
        let script = format!("printf '%d' '{shape}'");
        let probe = bash.exec(&script).await.expect("printf runs");
        assert!(
            probe.stderr.contains(shape),
            "precondition: the diagnostic must echo the operand; stderr: {:?}",
            probe.stderr.to_string()
        );
        fuzz_exec(&mut bash, &script, "printf_fuzz", &[]).await;
    }
}

/// `awk_fuzz` run 310: the awk program was `{ code:…`, and
/// `awk: cmd. line:1: {program}` prefixes it with `line:1: `. The Debug shape
/// ` { code:` is spelled across that join — the leading space is the
/// template's, the rest is the program — so it appears nowhere in the script.
/// Whole-script padding cannot see an interior join, which is why
/// `script_explains` pads the *pattern* instead.
#[tokio::test]
async fn awk_crash_code_shape_across_template_join_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("eyBjb2RlOgAAAAAAAABobw==");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let (program, input_data) = match input.find('\n') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => (input, "a b c\n1 2 3\n"),
    };
    let mut bash = bash(4096);
    for script in [
        format!("echo '{}' | awk '{}'", sq(input_data), sq(program)),
        format!("echo '{}' | awk -F: '{}'", sq(input_data), sq(program)),
    ] {
        fuzz_exec(&mut bash, &script, "awk_fuzz", &[]).await;
    }
}

/// `printf_fuzz` run 310: the same interior join one builtin further on.
/// `printf: {arg}: invalid number` prefixes the operand with `printf: `, so an
/// operand of `{ code:` spells ` { code:`. The target's
/// `input_echo_would_trip` gate pads the whole input and so misses it too;
/// attribution in the harness is what has to cover this.
#[tokio::test]
async fn printf_crash_code_shape_across_template_join_is_an_echo_not_a_leak() {
    fuzz_init();
    let data = b64("JWNgY2B7IGNvZGU6Cgp7IGNvZGU6Cgo=");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let (format_str, args_str) = match input.find('\n') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => (input, ""),
    };
    let args_joined = args_str
        .lines()
        .map(|a| format!("'{}'", sq(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut bash = bash(4096);
    for script in [
        format!("printf '{}' {}", sq(format_str), args_joined),
        format!("printf -v result '{}' {}", sq(format_str), args_joined),
        format!("printf '%d %o %x' {args_joined}"),
    ] {
        fuzz_exec(&mut bash, &script, "printf_fuzz", &[]).await;
    }
}

/// `tomlq_fuzz` run 310: the query held `target/debug/\x1f\x1f\x1fdeps/`, which
/// is *not* the banned host path — until the diagnostic sanitizes the control
/// bytes out of the quoted operand and `tomlq: path '…' not found` reads
/// `target/debug/deps/`. Attribution compares against the raw script, so it
/// has to account for the same sanitization the renderer applies.
#[tokio::test]
async fn tomlq_crash_control_bytes_sanitized_into_host_path_is_an_echo() {
    fuzz_init();
    let data = b64("UwAAAAAnJ0BAQEAnCicAAAAAAAoAdGFyZ2V0L2RlYnVnLx8fH2RlcHMvOgE=");
    let input = std::str::from_utf8(&data).expect("crash unit is UTF-8");
    let (toml_doc, query) = match input.find('\n') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => (input, "."),
    };
    let mut bash = bash(4096);
    for script in [
        format!("echo '{}' | tomlq '{}'", sq(toml_doc), sq(query)),
        format!("echo '{}' | tomlq", sq(toml_doc)),
    ] {
        fuzz_exec(&mut bash, &script, "tomlq_fuzz", &[]).await;
    }
}
