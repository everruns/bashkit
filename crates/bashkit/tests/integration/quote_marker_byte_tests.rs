//! 0x1e/0x1f in script text are data, not the lexer's quote-boundary markers.
//!
//! `parser::lexer` marks the quoted spans of a mixed word with in-band
//! `\u{1e}`/`\u{1f}` markers that `parse_word` turns into per-part quotedness
//! and then consumes. A byte that spells a marker but came from the *script*
//! has to survive that round trip behind the NUL literal sentinel
//! (`lexer::push_literal_char`); unescaped it was silently deleted, so
//! `printf '[%s]' A<1f>B` printed `[AB]` where bash prints `[A<1f>B]`.
//!
//! 0x1e and 0x1f are RS and US, ordinary separators in real data
//! (`IFS=$'\x1f'`, `awk -F$'\x1e'`), so this is reachable without any
//! adversarial intent. It also broke the fuzz harness' leak attribution,
//! which compares a builtin's stderr against the script that produced it:
//! nightly `tomlq_fuzz` ran `tomlq 'target/debug/<1f><1f><1f>deps/'` and the
//! diagnostic quoted back the banned host path `target/debug/deps/`, a shape
//! the script did not spell. See `bashkit::testing::script_explains` and
//! `tests/fuzz_crash_regressions.rs`.

use bashkit::Bash;

/// Every quoting context that copies literal source characters into a word,
/// for both markers. `printf '[%s]'` brackets each field, so a dropped byte
/// and a split field are both visible.
fn marker_cases() -> Vec<String> {
    let mut cases = Vec::new();
    for m in ['\u{1e}', '\u{1f}'] {
        cases.extend([
            // Single-, double- and unquoted word text.
            format!("printf '[%s]' 'A{m}B'"),
            format!("printf '[%s]' \"A{m}B\""),
            format!("printf '[%s]' A{m}B"),
            // Concatenated segments: the continuation reader has its own
            // single-quote, backslash and unquoted paths.
            format!("printf '[%s]' \"A{m}B\"'C{m}D'"),
            format!("printf '[%s]' 'A{m}B'\"C{m}D\""),
            format!("printf '[%s]' 'A'{m}'B'"),
            format!("printf '[%s]' ''\\''A{m}B'"),
            format!("printf '[%s]' \\{m}'A{m}B'"),
            format!(r"printf '[%s]' A\{m}B"),
            format!(r#"printf '[%s]' "A\{m}B""#),
            format!(r"printf '[%s]' \{m}"),
            // A `${...}` operand and a pattern operator inside one.
            format!(r#"x=aA{m}Bz; printf '[%s]' "${{x#aA{m}}}""#),
            format!(r#"x=q; printf '[%s]' "${{x:+A{m}B}}""#),
            // Backslash-escaped marker in a quoted `${...}` operand: the
            // reader keeps the backslash and the byte, as bash does. The
            // double-quoted-span form is in
            // `backslash_escaped_markers_take_the_ordinary_character_path`,
            // because bashkit's backslash handling there already diverges
            // from bash for every character.
            format!(r#"unset x; printf '[%s]' "${{x:-A\{m}B}}""#),
            // Quoted spans inside a `${...}` operand. The plain `'...'` span
            // needs the literal sentinel; the ANSI-C `$'...'` span is decoded
            // again downstream and must NOT get one, or a raw NUL leaks into
            // the value. Both directions are regressions this pins.
            format!(r#"unset x; printf '[%s]' "${{x:-'A{m}B'}}""#),
            format!(r#"unset x; printf '[%s]' "${{x:-$'A{m}B'}}""#),
            format!(r#"unset x; printf '[%s]' "${{x:-"A{m}B"}}""#),
            format!(r#"unset x; printf '[%s]' ${{x:-'A{m}B'}}"#),
            format!(r#"unset x; printf '[%s]' ${{x:-$'A{m}B'}}"#),
            // Pattern and replacement operands around the byte.
            format!(r#"x=aA{m}Bz; printf '[%s]' "${{x%%B{m}z}}""#),
            format!(r#"x=A{m}B; printf '[%s]' "${{x/{m}/-}}""#),
            format!(r#"x=A{m}B; printf '[%s]' "${{#x}}""#),
            // A brace tail on a word carrying the byte (the unquoted-glob
            // path). Extglob is deliberately not covered here: `shopt -s
            // extglob` in the same `-c` string takes effect after bash has
            // already parsed it, so the oracle would not agree with itself.
            format!("printf '[%s]' A{m}{{1,2}}"),
            // ANSI-C quoting, already protected; kept so a regression there
            // fails here too.
            format!("printf '[%s]' $'A{m}B'"),
            // Through a variable, quoted and unquoted, and a `${...}` form.
            format!("x=A{m}B; printf '[%s]' \"$x\""),
            format!("x=A{m}B; printf '[%s]' $x"),
            format!("x=A{m}B; printf '[%s]' \"${{x}}\""),
            // Command substitution output is data, never rescanned for markers.
            format!("printf '[%s]' \"$(printf 'A{m}B')\""),
            // A nested body is copied verbatim and re-lexed when it runs, so a
            // literal marker inside it survives that second pass.
            format!("echo $(echo 'A{m}B')"),
            format!("echo \"$(echo 'A{m}B')\""),
            format!("echo `echo 'A{m}B'`"),
            format!("echo $(printf '[%s]' \"A{m}B\")"),
            // A word that also globs keeps the unquoted-glob path.
            format!("printf '[%s]' A{m}B*"),
            // Array element text. Subscripts are deliberately absent: see
            // `subscript_text_keeps_its_pre_existing_shape` below.
            format!("a=(A{m}B C); printf '[%s]' \"${{a[@]}}\""),
            // Heredoc bodies go through `heredoc_body_escapes`.
            format!("cat <<EOF\nA{m}B\nEOF"),
            format!("cat <<'EOF'\nA{m}B\nEOF"),
            // Pattern and conditional contexts.
            format!("if [[ A{m}B == A{m}B ]]; then echo yes; else echo no; fi"),
            format!("case A{m}B in A{m}B) echo hit ;; *) echo miss ;; esac"),
            // IFS split on a marker: the classic real-data idiom.
            format!(
                "IFS=$'\\x{:02x}'; printf '[%s]' $(printf 'a{m}b')",
                m as u32
            ),
        ]);
    }
    cases
}

/// Real bash is the oracle. Every script is pure builtins, variables and
/// heredocs — no host I/O — so the comparison is hermetic.
#[tokio::test]
#[cfg_attr(not(unix), ignore = "compares against host Unix bash")]
async fn marker_bytes_are_word_data_matching_bash() {
    if std::process::Command::new("bash")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skip: bash is not installed");
        return;
    }
    for script in marker_cases() {
        let host = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", &script])
            .env_clear()
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        let actual = Bash::new().exec(&script).await.unwrap();
        assert_eq!(
            actual.stdout.as_bytes(),
            host.stdout,
            "stdout differs for {script:?}\n  bashkit: {:?}\n  bash:    {:?}",
            actual.stdout.to_string(),
            String::from_utf8_lossy(&host.stdout),
        );
        assert_eq!(
            actual.exit_code,
            host.status.code().unwrap_or(-1),
            "exit differs for {script:?}: {}",
            actual.stderr
        );
    }
}

/// The byte reaches a builtin's argv intact, so a diagnostic that quotes its
/// operand quotes the real bytes. This is the property the fuzz harness'
/// attribution depends on: stderr may not spell anything the script did not.
#[tokio::test]
async fn builtin_diagnostics_quote_marker_bytes_verbatim() {
    let mut bash = Bash::new();
    let result = bash
        .exec("echo 'x=1' | tomlq 'target/debug/\u{1f}\u{1f}\u{1f}deps/'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .stderr
            .contains("target/debug/\u{1f}\u{1f}\u{1f}deps/"),
        "the operand must be echoed verbatim, got {:?}",
        result.stderr.to_string()
    );
    assert!(
        !result.stderr.contains("target/debug/deps/"),
        "dropping the markers invents a host path the script never spelled: {:?}",
        result.stderr.to_string()
    );
}

/// TODO: a literal marker byte inside an array *subscript* still diverges from
/// bash — `m[k<1e>1]=v` stores the key as `k1`, so `${m[k<1e>1]}` finds
/// nothing, and bash stores and reads `k<1e>1`. That is pre-existing, not a
/// consequence of `push_literal_char`: subscript text is handed to arithmetic
/// or associative-key expansion, which resolve quoting themselves, and
/// `try_parse_assignment` additionally recovers a marker-bearing subscript from
/// raw source. Escaping it in the lexer alone makes assignment and lookup
/// disagree, so the readers copy subscript text verbatim and this case is left
/// exactly as it was. Fixing it means teaching those three layers the sentinel
/// together.
///
/// This test pins the current shape so the next change here is deliberate.
#[tokio::test]
async fn subscript_text_keeps_its_pre_existing_shape() {
    let mut bash = Bash::new();
    let stored = bash
        .exec("declare -A m; m[k\u{1e}1]=V; printf '[%s]' \"${!m[@]}\"")
        .await
        .unwrap();
    assert_eq!(
        stored.stdout.to_string(),
        "[k1]",
        "a literal marker in a subscript is still dropped at assignment"
    );
    let hit = bash
        .exec("declare -A m; m[k\u{1e}1]=V; printf '[%s]' \"${m[k1]}\"")
        .await
        .unwrap();
    assert_eq!(hit.stdout.to_string(), "[V]", "under the normalized key");
    // The `$'...'` spelling has always worked and must keep working: it is the
    // form anyone separating real data actually writes.
    let ansi = bash
        .exec("declare -A m; k=$'k\\x1e1'; m[$k]=V; printf '[%s]' \"${m[$k]}\"")
        .await
        .unwrap();
    assert_eq!(ansi.stdout.to_string(), "[V]");
}

/// Extglob group bodies keep the byte too. bashkit parses `@(...)` as a group
/// without `shopt -s extglob`, so the oracle runs with `-O extglob` to compare
/// like with like — plain `bash -c` makes `@(` a syntax error instead.
#[tokio::test]
#[cfg_attr(not(unix), ignore = "compares against host Unix bash")]
async fn extglob_group_bodies_keep_marker_bytes() {
    if std::process::Command::new("bash")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skip: bash is not installed");
        return;
    }
    for m in ['\u{1e}', '\u{1f}'] {
        for script in [
            format!("printf '[%s]' @(A{m}B)"),
            format!("printf '[%s]' x@(A{m}B)y"),
            format!("printf '[%s]' @(A{m}B|zz)"),
            // Quoted spans inside the group go through `push_extglob_literal`.
            format!("printf '[%s]' @('A{m}B')"),
            format!("printf '[%s]' @(\"A{m}B\")"),
            // A group after a quoted start is read by the continuation
            // reader's own extglob loop, not `read_word_from`'s.
            format!("printf '[%s]' \"x\"*@(A{m}B)"),
            format!("printf '[%s]' 'p'*@(A{m}B)"),
        ] {
            let host = std::process::Command::new("bash")
                .args(["--noprofile", "--norc", "-O", "extglob", "-c", &script])
                .env_clear()
                .env("LC_ALL", "C")
                .output()
                .unwrap();
            let actual = Bash::new().exec(&script).await.unwrap();
            assert_eq!(
                actual.stdout.as_bytes(),
                host.stdout,
                "stdout differs for {script:?}\n  bashkit: {:?}\n  bash:    {:?}",
                actual.stdout.to_string(),
                String::from_utf8_lossy(&host.stdout),
            );
        }
    }
}

/// Backslash-escaped markers in `${...}` operands survive and take exactly
/// the path an ordinary character takes.
///
/// Not compared with bash: bashkit's backslash handling in these two readers
/// already diverges from bash for *every* character, not just these bytes —
/// `${x:-A\ZB}` unquoted is `A\ZB` here and `AZB` in bash, and
/// `"${x:-"A\ZB"}"` is `A\\ZB` here and `AZB` in bash. Those are
/// pre-existing and independent of marker handling, so what this pins is the
/// property this change is about: a marker byte is data like any other byte,
/// never silently deleted.
#[tokio::test]
async fn backslash_escaped_markers_take_the_ordinary_character_path() {
    let mut bash = Bash::new();
    for (quoted_label, marker_script, plain_script) in [
        (
            "unquoted operand",
            r#"unset x; printf '[%s]' ${x:-A\MB}"#,
            r#"unset x; printf '[%s]' ${x:-A\ZB}"#,
        ),
        (
            "double-quoted span inside the operand",
            r#"unset x; printf '[%s]' "${x:-"A\MB"}""#,
            r#"unset x; printf '[%s]' "${x:-"A\ZB"}""#,
        ),
        (
            "backslash inside a continuation extglob group",
            r#"printf '[%s]' "x"*@(A\MB)"#,
            r#"printf '[%s]' "x"*@(A\ZB)"#,
        ),
    ] {
        for m in ['\u{1e}', '\u{1f}'] {
            let marked = marker_script.replace('M', &m.to_string());
            let got = bash.exec(&marked).await.unwrap().stdout.to_string();
            assert!(
                got.contains(m),
                "{quoted_label}: the byte must survive {marked:?}; got {got:?}"
            );
            let plain = bash.exec(plain_script).await.unwrap().stdout.to_string();
            assert_eq!(
                got.replace(m, "Z"),
                plain,
                "{quoted_label}: a marker must take the same path as any other character"
            );
        }
    }
}

/// The literal sentinel must never reach a value. `push_literal_char` escapes
/// a marker byte for `parse_word`, but some readers hand their span to a
/// second decode that does not consume the sentinel — the ANSI-C `$'...'` span
/// inside `${...}` is one, where escaping turned
/// `"${x:-$'A<1f>B'}"` into `A\0<1f>B`. Any stray NUL in output means a
/// sentinel was added on a path that does not decode it.
#[tokio::test]
async fn the_literal_sentinel_never_leaks_into_a_value() {
    let mut bash = Bash::new();
    for m in ['\u{1e}', '\u{1f}'] {
        for script in [
            format!(r#"unset x; printf '[%s]' "${{x:-$'A{m}B'}}""#),
            format!(r#"unset x; printf '[%s]' "${{x:-'A{m}B'}}""#),
            format!(r#"unset x; printf '[%s]' "${{x:-"A{m}B"}}""#),
            format!(r#"unset x; printf '[%s]' ${{x:-$'A{m}B'}}"#),
            format!("printf '[%s]' $'A{m}B'"),
            format!("printf '[%s]' \"A{m}B\""),
            format!("printf '[%s]' @('A{m}B')"),
        ] {
            let out = bash.exec(&script).await.unwrap().stdout.to_string();
            assert!(
                !out.contains('\0'),
                "a literal sentinel leaked for {script:?}: {out:?}"
            );
            assert!(
                out.contains(m),
                "the byte must survive for {script:?}: {out:?}"
            );
        }
    }
}
