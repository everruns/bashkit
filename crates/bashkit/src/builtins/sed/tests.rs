use super::*;
use crate::fs::{FileSystem, InMemoryFs};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

async fn run_with(
    fs: Arc<InMemoryFs>,
    args: &[&str],
    stdin: Option<&str>,
) -> crate::interpreter::ExecResult {
    let sed = Sed;
    let mut vars = HashMap::new();
    let mut cwd = PathBuf::from("/");
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let ctx = Context {
        args: &args,
        env: &HashMap::new(),
        variables: &mut vars,
        cwd: &mut cwd,
        fs,
        stdin: crate::builtins::test_stream_opt(stdin),
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };
    sed.execute(ctx)
        .await
        .expect("sed must not fail the interpreter")
}

async fn run_sed(args: &[&str], stdin: Option<&str>) -> crate::interpreter::ExecResult {
    run_with(Arc::new(InMemoryFs::new()), args, stdin).await
}

async fn out(args: &[&str], stdin: &str) -> String {
    run_sed(args, Some(stdin)).await.stdout.to_string()
}

async fn stdout_of(args: &[&str], stdin: &str) -> String {
    out(args, stdin).await
}

// === regression coverage for the original builtin =======================

#[tokio::test]
async fn substitute_basics() {
    assert_eq!(
        out(&["s/hello/goodbye/"], "hello world\nhello again\n").await,
        "goodbye world\ngoodbye again\n"
    );
    assert_eq!(out(&["s/o/0/g"], "hello world\n").await, "hell0 w0rld\n");
    assert_eq!(out(&["s/o/0/"], "hello world\n").await, "hell0 world\n");
    assert_eq!(
        out(&["s/world/[&]/"], "hello world\n").await,
        "hello [world]\n"
    );
    assert_eq!(out(&["s/hello/hi/i"], "Hello World\n").await, "hi World\n");
    assert_eq!(
        out(&["s/hello/hi/; s/world/there/"], "hello world\n").await,
        "hi there\n"
    );
}

#[tokio::test]
async fn addresses_and_hold_space() {
    assert_eq!(
        out(&["2d"], "line1\nline2\nline3\n").await,
        "line1\nline3\n"
    );
    assert_eq!(out(&["-n", "2p"], "line1\nline2\nline3\n").await, "line2\n");
    assert_eq!(
        out(&["2,3d"], "line1\nline2\nline3\nline4\n").await,
        "line1\nline4\n"
    );
    assert_eq!(
        out(&["$d"], "line1\nline2\nline3\n").await,
        "line1\nline2\n"
    );
    assert_eq!(
        out(&["2c\\replaced"], "line1\nline2\nline3\n").await,
        "line1\nreplaced\nline3\n"
    );
    assert_eq!(
        out(&["/one/a\\inserted"], "one\ntwo\n").await,
        "one\ninserted\ntwo\n"
    );
    assert_eq!(
        out(&["/two/i\\inserted"], "one\ntwo\n").await,
        "one\ninserted\ntwo\n"
    );
    assert_eq!(
        out(&["-e", "1h", "-e", "2g"], "first\nsecond\nthird\n").await,
        "first\nfirst\nthird\n"
    );
    assert_eq!(
        out(&["-e", "1h", "-e", "2H", "-e", "3G"], "a\nb\nc\n").await,
        "a\nb\nc\na\nb\n"
    );
    assert_eq!(
        out(&["-e", "1h", "-e", "2x"], "first\nsecond\nthird\n").await,
        "first\nfirst\nthird\n"
    );
    assert_eq!(
        out(&["/start/,/end/d"], "before\nstart\nmiddle\nend\nafter\n").await,
        "before\nafter\n"
    );
    assert_eq!(
        out(&["/begin/,/end/s/x/y/g"], "ax\nbeginx\nmiddlex\nendx\nax\n").await,
        "ax\nbeginy\nmiddley\nendy\nax\n"
    );
}

#[tokio::test]
async fn back_references_use_the_fancy_engine() {
    assert_eq!(
        out(&["s/\\(hello\\) \\(world\\)/\\2 \\1/"], "hello world\n").await,
        "world hello\n"
    );
    assert_eq!(out(&["s/\\(.\\)\\1/X/g"], "aabbc\n").await, "XXc\n");
    assert_eq!(out(&["s/\\(hel\\)lo/\\1p/"], "hello\n").await, "help\n");
    assert_eq!(
        out(
            &[r#"s|<a href="tag_\([^"]*\)">\1</a>|\1|g"#],
            "<a href=\"tag_hello\">hello</a>\n"
        )
        .await,
        "hello\n"
    );
}

#[test]
fn fancy_regex_backtrack_limit_is_enforced() {
    let Ok(pattern::SedRegex::Fancy(re)) =
        pattern::SedRegex::build_fancy_with_limit(r"(a+)+\1b", false, false, 100)
    else {
        panic!("expected fancy-regex fallback");
    };
    let err = re
        .is_match("a".repeat(5_000).as_str())
        .expect_err("expected backtrack limit error");
    assert!(!err.to_string().is_empty());
}

#[tokio::test]
async fn grouped_command_nesting_depth_is_capped() {
    let depth = crate::builtins::limits::SED_MAX_GROUP_NESTING_DEPTH + 1;
    let script = format!("{}p{}", "{".repeat(depth), "}".repeat(depth));
    let result = run_sed(&[&script], Some("a\n")).await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("nesting exceeds max depth"),
        "{}",
        result.stderr
    );
}

#[tokio::test]
async fn branch_loop_limit_emits_a_warning() {
    let result = run_sed(&[":loop; s/a/aa/; /a\\{2000\\}/!b loop"], Some("a\n")).await;
    assert!(result.stderr.contains("loop limit"), "{}", result.stderr);
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn ordinary_scripts_write_nothing_to_stderr() {
    let result = run_sed(&["s/hello/world/"], Some("hello\n")).await;
    assert!(result.stderr.is_empty());
    assert_eq!(result.stdout, "world\n");
}

// === finding A: `$` in the replacement is literal =======================

#[tokio::test]
async fn dollar_in_replacement_is_literal() {
    assert_eq!(out(&["s/a/$x/"], "ab\n").await, "$xb\n");
    assert_eq!(out(&["s/\\(a\\)/[$1]/"], "ab\n").await, "[$1]b\n");
    assert_eq!(out(&["s/a/$$/"], "ab\n").await, "$$b\n");
    assert_eq!(out(&["s/a/${x}/"], "ab\n").await, "${x}b\n");
    // GNU drops the backslash before an ordinary character.
    assert_eq!(out(&["s/a/\\$/"], "ab\n").await, "$b\n");
    assert_eq!(out(&["s/a/\\q/"], "ab\n").await, "qb\n");
    assert_eq!(out(&["s/^/$PREFIX-/"], "x\n").await, "$PREFIX-x\n");
}

#[tokio::test]
async fn replacement_case_conversion() {
    assert_eq!(
        out(&["s/a\\(b\\)c/\\U\\1x\\E-\\1/"], "abc\n").await,
        "BX-b\n"
    );
    assert_eq!(out(&["s/.*/\\u&/"], "abc\n").await, "Abc\n");
    assert_eq!(out(&["s/.*/\\L&/"], "ABC\n").await, "abc\n");
}

// === finding B: BRE literals vs ERE operators ===========================

#[tokio::test]
async fn bre_treats_plus_question_pipe_as_literals() {
    assert_eq!(out(&["s/a+b/X/"], "a+b\n").await, "X\n");
    assert_eq!(out(&["s/a?b/X/"], "a?b\n").await, "X\n");
    assert_eq!(out(&["s/a|b/X/"], "a|b\n").await, "X\n");
    assert_eq!(out(&["s/a{b/X/"], "a{b\n").await, "X\n");
    assert_eq!(out(&["s/a(b)/X/"], "a(b)\n").await, "X\n");
    // A leading `*` is an ordinary character, not a dangling quantifier.
    assert_eq!(out(&["s/*a/X/"], "aaa\n").await, "aaa\n");
    // `^` and `$` only anchor at the edges.
    assert_eq!(out(&["-n", "/a^b/p"], "a^b\n").await, "a^b\n");
    assert_eq!(out(&["-n", "/a$b/p"], "a$b\n").await, "a$b\n");
}

#[tokio::test]
async fn address_regexes_go_through_the_same_bre_translation() {
    assert_eq!(out(&["-n", "/a\\+/p"], "aaa\n").await, "aaa\n");
    assert_eq!(out(&["-n", "/a\\|b/p"], "b\n").await, "b\n");
    assert_eq!(out(&["-n", "/a+b/p"], "a+b\n").await, "a+b\n");
    assert_eq!(out(&["/x\\?/d"], "x?\n").await, "");
}

#[tokio::test]
async fn extended_mode_keeps_ere_operators() {
    assert_eq!(out(&["-E", "s/a+/X/"], "aaa\n").await, "X\n");
    assert_eq!(out(&["-E", "s/a(X)b/[\\1]/"], "aXb\n").await, "[X]\n");
    assert_eq!(out(&["-E", "s/a\\+b/X/"], "a+b\n").await, "X\n");
}

#[tokio::test]
async fn bracket_expressions_keep_posix_semantics() {
    assert_eq!(out(&["s/[]]/X/"], "a]b\n").await, "aXb\n");
    assert_eq!(out(&["s/[/]/X/"], "a/b\n").await, "aXb\n");
    assert_eq!(out(&["s/[a-c]/X/g"], "a-b\n").await, "X-X\n");
    assert_eq!(out(&["s/[[:alpha:]]/X/2"], "abc\n").await, "aXc\n");
    // POSIX gives `\` no special meaning inside a bracket expression.
    assert_eq!(out(&["s/[\\]/X/g"], "a\\b\n").await, "aXb\n");
}

#[test]
fn translate_is_positional() {
    assert_eq!(pattern::translate("a+b", false), "a\\+b");
    assert_eq!(pattern::translate("a\\+b", false), "a+b");
    assert_eq!(pattern::translate("^a$", false), "^a$");
    assert_eq!(pattern::translate("a^b$c", false), "a\\^b\\$c");
    assert_eq!(pattern::translate("*a", false), "\\*a");
    assert_eq!(pattern::translate("a*", false), "a*");
    assert_eq!(pattern::translate("a+b", true), "a+b");
}

// === finding C: range addresses =========================================

#[tokio::test]
async fn regex_to_line_ranges_keep_the_regex() {
    assert_eq!(out(&["/b/,3d"], "a\nb\nc\nd\n").await, "a\nd\n");
    // A numeric end at or before the start line makes a one-line range.
    assert_eq!(out(&["-n", "/b/,1p"], "a\nb\nc\nd\n").await, "b\n");
}

#[tokio::test]
async fn a_closed_range_stays_closed() {
    assert_eq!(out(&["-n", "1,/b/p"], "a\nb\nc\n").await, "a\nb\n");
    // A line-number start tests the end regex from the *next* line ...
    assert_eq!(out(&["-n", "1,/b/p"], "b\nb\nc\n").await, "b\nb\n");
    // ... while `0,/re/` tests it on the first line.
    assert_eq!(out(&["-n", "0,/b/p"], "b\nb\nc\n").await, "b\n");
    // A regex start may re-open later.
    assert_eq!(
        out(&["-n", "/a/,/b/p"], "a\nb\na\nb\nc\n").await,
        "a\nb\na\nb\n"
    );
}

/// A relative end (`+N`, `~N`) is recomputed on each activation, so a stream
/// carried past it by `n`/`N`/`D`/`b` re-arms the start; an absolute `,N` end
/// is spent for good. Both shapes are GNU behaviour.
#[tokio::test]
async fn ranges_that_the_stream_skipped_past() {
    // `1~2N` advances the line counter mid-cycle, so `2,+1` is evaluated at
    // lines 2 and 4 and opens both times.
    assert_eq!(
        out(&["-n", "1~2N;2,+1a\\APP"], "a\nb\nc\nd\ne\n").await,
        "APP\nAPP\n"
    );
    // An absolute end does not re-arm: `1,2` is spent once line 3 is reached.
    assert_eq!(
        out(&["-s", "1,2n;1,/b/s/^/>/"], "a\nb\nc\n").await,
        "a\n>b\n>c\n"
    );
    // A range is only re-tested from its start once the end has been passed.
    assert_eq!(out(&["-n", "1b;1,/zz/p"], "a\nb\nc\n").await, "b\nc\n");
    assert_eq!(out(&["-n", "1b;1,2p"], "a\nb\nc\n").await, "b\n");
    assert_eq!(out(&["-n", "1,2b;1,3p"], "a\nb\nc\n").await, "c\n");
    assert_eq!(
        out(&["2,~3b;2,~3p"], "a\nb\nc\nd\ne\n").await,
        "a\nb\nc\nd\nd\ne\ne\n"
    );
}

#[tokio::test]
async fn hold_space_carries_its_own_newline_flag() {
    // The hold space starts out terminated, so `G` re-terminates a final line
    // that arrived without a newline.
    assert_eq!(out(&["G"], "ab").await, "ab\n\n");
    assert_eq!(out(&["x"], "a\nb\n").await, "\na\n");
    assert_eq!(
        out(&["-n", "1!G;h;$p"], "a\nb\nc\nd\n").await,
        "d\nc\nb\na\n"
    );
}

#[tokio::test]
async fn quit_terminates_the_line_it_printed() {
    // Falling off the end of input keeps a missing newline missing, but `q`
    // flushes it; `Q`, which prints nothing, does not.
    assert_eq!(out(&["q"], "ab").await, "ab\n");
    assert_eq!(out(&["-n", "p;q"], "ab").await, "ab\n");
    assert_eq!(out(&["-n", "p"], "ab").await, "ab");
    assert_eq!(out(&["Q"], "ab").await, "");
    assert_eq!(out(&["2q"], "a\nb").await, "a\nb\n");
}

#[tokio::test]
async fn relative_and_multiple_end_addresses() {
    let input = (1..=12).map(|n| format!("{n}\n")).collect::<String>();
    assert_eq!(out(&["-n", "2,~4p"], &input).await, "2\n3\n4\n");
    assert_eq!(out(&["-n", "4,~4p"], &input).await, "4\n5\n6\n7\n8\n");
    assert_eq!(out(&["-n", "3,+2p"], &input).await, "3\n4\n5\n");
    assert_eq!(out(&["-n", "$,$p"], "a\nb\nc\n").await, "c\n");
}

#[tokio::test]
async fn change_on_a_range_emits_its_text_once() {
    assert_eq!(out(&["1,2c\\Z"], "a\nb\nc\n").await, "Z\nc\n");
    assert_eq!(out(&["1,2!c\\Z"], "a\nb\nc\n").await, "a\nb\nZ\n");
    // A range that never matches emits nothing.
    assert_eq!(out(&["/zz/,/b/c\\Z"], "a\nb\nc\n").await, "a\nb\nc\n");
    assert_eq!(out(&["2c\\Z"], "a\nb\nc\n").await, "a\nZ\nc\n");
    // `c` ignores -n.
    assert_eq!(out(&["-n", "1,2c\\Z"], "a\nb\nc\n").await, "Z\n");
    // Running off the end of input does not close a range, so no text fires.
    assert_eq!(out(&["1,5c\\Z"], "a\nb\n").await, "");
    assert_eq!(out(&["2,5c\\Z"], "a\nb\nc\n").await, "a\n");
    assert_eq!(out(&["/a/,/zz/c\\Z"], "a\nb\nc\n").await, "");
    // Inside a block the `c` carries no address of its own, so it fires per line.
    assert_eq!(out(&["1,2{c\\Z\n}"], "a\nb\nc\n").await, "Z\nZ\nc\n");
}

#[tokio::test]
async fn quit_rejects_a_second_address() {
    for script in ["1,2q", "2,+1q", "1,2Q", "/a/,/b/q"] {
        let result = run_sed(&[script], Some("a\nb\n")).await;
        assert_eq!(result.exit_code, 1, "{script}");
        assert!(
            result.stderr.contains("command only uses one address"),
            "{script}: {}",
            result.stderr
        );
    }
    // Commands that GNU does allow two addresses on still work.
    assert_eq!(out(&["-n", "1,2="], "a\nb\nc\n").await, "1\n2\n");
    assert_eq!(out(&["1,2a\\X"], "a\nb\n").await, "a\nX\nb\nX\n");
}

#[tokio::test]
async fn back_references_beyond_the_group_count_are_rejected() {
    for script in [r"s/a/\1/", r"s/\(a\)/\2/", r"s/a/[\9]/"] {
        let result = run_sed(&[script], Some("a\n")).await;
        assert_eq!(result.exit_code, 1, "{script}");
        assert!(
            result.stderr.contains("invalid reference"),
            "{script}: {}",
            result.stderr
        );
    }
    // In ERE mode `\(` is a literal paren, so there is no group 1 to reference.
    let ere = run_sed(&["-E", r"s/\(a\)b/[\1]/"], Some("a\n")).await;
    assert_eq!(ere.exit_code, 1);
    assert!(ere.stderr.contains("invalid reference"), "{}", ere.stderr);
    // A real group is fine.
    assert_eq!(out(&["s/\\(a\\)/[\\1]/"], "ab\n").await, "[a]b\n");
}

/// THREAT[TM-DOS]: `D` reruns the script without reading input, so `G;D` never
/// terminates and grows the pattern space without bound. GNU hangs here; the
/// sandbox must not.
#[tokio::test]
async fn a_non_progressing_delete_restart_loop_terminates() {
    let result = run_sed(&["G;D"], Some("a\nb\n")).await;
    assert_eq!(result.exit_code, 0);
    assert!(result.stderr.contains("loop limit"), "{}", result.stderr);
    // A `D` loop that does make progress still terminates on its own.
    let progressing = run_sed(&["-s", "1,2!h;2,+1G;2,$D"], Some("abc\nABC\n")).await;
    assert_eq!(progressing.exit_code, 0);
    assert_eq!(progressing.stdout, "abc\n\n\n");
    assert!(progressing.stderr.is_empty(), "{}", progressing.stderr);
}

// === finding D: operands are one stream =================================

async fn fs_with_two_files() -> Arc<InMemoryFs> {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/f1"), b"a\nb\nc\n")
        .await
        .unwrap();
    fs.write_file(std::path::Path::new("/f2"), b"d\ne\n")
        .await
        .unwrap();
    fs
}

#[tokio::test]
async fn multiple_files_are_one_continuous_stream() {
    let fs = fs_with_two_files().await;
    let last = run_with(fs.clone(), &["-n", "$p", "/f1", "/f2"], None).await;
    assert_eq!(last.stdout, "e\n");
    let quit = run_with(fs.clone(), &["2q", "/f1", "/f2"], None).await;
    assert_eq!(quit.stdout, "a\nb\n");
    let first = run_with(fs.clone(), &["-n", "1p", "/f1", "/f2"], None).await;
    assert_eq!(first.stdout, "a\n");
    let count = run_with(fs.clone(), &["-n", "$=", "/f1", "/f2"], None).await;
    assert_eq!(count.stdout, "5\n");
}

#[tokio::test]
async fn separate_restores_per_file_streams() {
    let fs = fs_with_two_files().await;
    let last = run_with(fs.clone(), &["-s", "-n", "$p", "/f1", "/f2"], None).await;
    assert_eq!(last.stdout, "c\ne\n");
    let names = run_with(fs.clone(), &["-n", "F", "/f1", "/f2"], None).await;
    assert_eq!(names.stdout, "/f1\n/f1\n/f1\n/f2\n/f2\n");
}

// === finding E: trailing newline and in-place writes ====================

#[tokio::test]
async fn a_missing_final_newline_is_not_invented() {
    assert_eq!(out(&["s/a/b/"], "a").await, "b");
    assert_eq!(out(&["p"], "a").await, "a\na");
    assert_eq!(out(&["-n", "p"], "a\nb").await, "a\nb");
    assert_eq!(out(&["s/a/b/"], "a\n").await, "b\n");
    assert_eq!(out(&[""], "").await, "");
}

#[tokio::test]
async fn in_place_preserves_bytes_and_mode() {
    let fs = Arc::new(InMemoryFs::new());
    let path = std::path::Path::new("/k.txt");
    fs.write_file(path, b"abc").await.unwrap();
    fs.chmod(path, 0o600).await.unwrap();

    let result = run_with(fs.clone(), &["-i", "s/b/X/", "/k.txt"], None).await;
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert_eq!(fs.read_file(path).await.unwrap(), b"aXc");
    assert_eq!(fs.stat(path).await.unwrap().mode, 0o600);
    // No temporary left behind.
    assert!(
        fs.read_dir(std::path::Path::new("/"))
            .await
            .unwrap()
            .iter()
            .all(|e| !e.name.starts_with(".bashkit-sed-"))
    );
}

#[tokio::test]
async fn in_place_with_suffix_keeps_a_backup() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/k.txt"), b"a\nb\n")
        .await
        .unwrap();
    let result = run_with(fs.clone(), &["-i.bak", "s/a/A/", "/k.txt"], None).await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        fs.read_file(std::path::Path::new("/k.txt")).await.unwrap(),
        b"A\nb\n"
    );
    assert_eq!(
        fs.read_file(std::path::Path::new("/k.txt.bak"))
            .await
            .unwrap(),
        b"a\nb\n"
    );
}

#[tokio::test]
async fn in_place_refuses_non_utf8_rather_than_corrupting() {
    let fs = Arc::new(InMemoryFs::new());
    let path = std::path::Path::new("/bin.dat");
    fs.write_file(path, &[0x61, 0xff, 0x62]).await.unwrap();
    let result = run_with(fs.clone(), &["-i", "s/a/X/", "/bin.dat"], None).await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("not a valid UTF-8 text file"),
        "{}",
        result.stderr
    );
    assert_eq!(fs.read_file(path).await.unwrap(), vec![0x61, 0xff, 0x62]);
}

#[tokio::test]
async fn in_place_without_operands_is_an_error() {
    let result = run_sed(&["-i", "s/a/b/"], Some("a\n")).await;
    assert_eq!(result.exit_code, 4);
    assert!(
        result.stderr.contains("no input files"),
        "{}",
        result.stderr
    );
}

// === finding F: `s///Ng` ================================================

#[tokio::test]
async fn occurrence_and_global_compose() {
    assert_eq!(out(&["s/L/x/2g"], "heLLo\n").await, "heLxo\n");
    assert_eq!(out(&["s/a/X/3g"], "aaa\n").await, "aaX\n");
    assert_eq!(out(&["s/a/X/3"], "aaaa\n").await, "aaXa\n");
    assert_eq!(out(&["s/a/X/4"], "aaa\n").await, "aaa\n");
    assert_eq!(out(&["s/o/0/3"], "hello world\n").await, "hello world\n");
}

// === finding G: the missing commands ====================================

#[tokio::test]
async fn previously_missing_commands_work() {
    assert_eq!(out(&["-n", "$="], "a\nb\n").await, "2\n");
    assert_eq!(out(&["N;s/\\n/ /"], "a\nb\nc\n").await, "a b\nc\n");
    assert_eq!(out(&["y/abc/xyz/"], "abc\n").await, "xyz\n");
    assert_eq!(out(&["-n", "/a/{n;p}"], "a\nb\nc\n").await, "b\n");
    // `T` branches when *no* substitution has happened yet.
    assert_eq!(out(&["T end;s/a/X/;:end"], "abc\n").await, "abc\n");
    assert_eq!(out(&["s/a/X/;T end;s/b/Y/;:end"], "abc\n").await, "XYc\n");
    assert_eq!(out(&["# comment"], "a\nb\n").await, "a\nb\n");
    assert_eq!(out(&["-n", "N;P"], "a\nb\n").await, "a\n");
    assert_eq!(out(&["z;s/^$/EMPTY/"], "abc\n").await, "EMPTY\n");
    assert_eq!(out(&["-n", "l"], "a\tb\\c\n").await, "a\\tb\\\\c$\n");
    // `#n` on the first line implies -n.
    assert_eq!(out(&["#n\np"], "a\n").await, "a\n");
}

#[tokio::test]
async fn delete_first_line_restarts_the_cycle() {
    // Classic "squeeze repeated blank lines" idiom.
    assert_eq!(out(&["/^$/{N;/^\\n$/D}"], "a\n\n\n\nb\n").await, "a\n\nb\n");
}

#[tokio::test]
async fn transliterate_rejects_unequal_lengths() {
    let result = run_sed(&["y/ab/x/"], Some("a\n")).await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("different lengths"),
        "{}",
        result.stderr
    );
}

#[tokio::test]
async fn read_and_write_commands_use_the_vfs() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/r.txt"), b"R1\nR2\n")
        .await
        .unwrap();

    let appended = run_with(fs.clone(), &["1r /r.txt"], Some("a\nb\n")).await;
    assert_eq!(appended.stdout, "a\nR1\nR2\nb\n");

    let per_line = run_with(fs.clone(), &["R /r.txt"], Some("a\nb\n")).await;
    assert_eq!(per_line.stdout, "a\nR1\nb\nR2\n");

    let written = run_with(fs.clone(), &["-n", "1w /out.txt"], Some("a\nb\n")).await;
    assert_eq!(written.exit_code, 0);
    assert_eq!(
        fs.read_file(std::path::Path::new("/out.txt"))
            .await
            .unwrap(),
        b"a\n"
    );

    let via_subst = run_with(fs.clone(), &["-n", "s/a/X/w /out2.txt"], Some("a\nb\n")).await;
    assert_eq!(via_subst.exit_code, 0);
    assert_eq!(
        fs.read_file(std::path::Path::new("/out2.txt"))
            .await
            .unwrap(),
        b"X\n"
    );
}

#[tokio::test]
async fn the_e_command_is_refused_not_silently_ignored() {
    for script in ["1e echo hi", "s/a/b/e"] {
        let result = run_sed(&[script], Some("a\n")).await;
        assert_eq!(result.exit_code, 1, "{script}");
        assert!(
            result.stderr.contains("not available"),
            "{script}: {}",
            result.stderr
        );
    }
}

#[tokio::test]
async fn quit_carries_an_exit_status() {
    let quit = run_sed(&["1q5"], Some("a\nb\n")).await;
    assert_eq!(
        (quit.stdout.to_string(), quit.exit_code),
        ("a\n".to_string(), 5)
    );
    let silent = run_sed(&["1Q3"], Some("a\nb\n")).await;
    assert_eq!(
        (silent.stdout.to_string(), silent.exit_code),
        (String::new(), 3)
    );
}

#[test]
fn list_format_wraps_and_escapes() {
    assert_eq!(exec::list_format("a\tb", 70), "a\\tb$");
    assert_eq!(exec::list_format("é", 70), "\\303\\251$");
    assert_eq!(
        exec::list_format(&"x".repeat(80), 70),
        format!("{}\\\n{}$", "x".repeat(69), "x".repeat(11))
    );
    assert_eq!(exec::list_format(&"x".repeat(3), 0), "xxx$");
}

// === finding H: multi-byte `s` delimiter (TM-UNI-002) ===================

#[tokio::test]
async fn multibyte_delimiters_do_not_panic() {
    // Documented superset over GNU, which rejects a multi-byte delimiter.
    assert_eq!(out(&["s≠a≠X≠"], "a\n").await, "X\n");
    assert_eq!(out(&["s😀a😀X😀g"], "aa\n").await, "XX\n");
    let truncated = run_sed(&["s≠a"], Some("a\n")).await;
    assert_eq!(truncated.exit_code, 1);
    assert!(
        truncated.stderr.contains("unterminated"),
        "{}",
        truncated.stderr
    );
}

// === finding I: option parsing ==========================================

#[tokio::test]
async fn short_options_cluster_and_take_arguments() {
    assert_eq!(out(&["-ne", "p"], "a\n").await, "a\n");
    assert_eq!(
        out(&["-n", "-e", "1p", "-e", "2p"], "a\nb\n").await,
        "a\nb\n"
    );
    assert_eq!(out(&["-nE", "s/a+/X/p"], "aa\n").await, "X\n");
    assert_eq!(out(&["--quiet", "-e", "p"], "x\n").await, "x\n");
    assert_eq!(out(&["--expression=s/x/y/"], "x\n").await, "y\n");
    assert_eq!(out(&["-n", "--expression", "p"], "x\n").await, "x\n");
}

/// Every long option and every accepted-but-inert short option, so none of
/// them can rot into "silently ignored" without a test noticing.
#[tokio::test]
async fn every_long_option_is_exercised() {
    assert_eq!(out(&["--silent", "-e", "p"], "x\n").await, "x\n");
    assert_eq!(out(&["--quiet", "-e", "p"], "x\n").await, "x\n");
    assert_eq!(out(&["--regexp-extended", "s/a+/X/"], "aa\n").await, "X\n");
    assert_eq!(out(&["--expression=s/x/y/"], "x\n").await, "y\n");
    assert_eq!(
        out(&["--line-length=5", "-n", "l"], "abcdefgh\n").await,
        "abcd\\\nefgh$\n"
    );
    // -u / --unbuffered and --follow-symlinks are accepted and inert.
    assert_eq!(out(&["-u", "s/a/b/"], "a\n").await, "b\n");
    assert_eq!(out(&["--unbuffered", "s/a/b/"], "a\n").await, "b\n");
    assert_eq!(out(&["--follow-symlinks", "s/a/b/"], "a\n").await, "b\n");
    assert_eq!(out(&["--posix", "s/a/b/"], "a\n").await, "b\n");
    assert_eq!(out(&["--debug", "s/a/b/"], "a\n").await, "b\n");
    assert_eq!(out(&["--sandbox", "s/a/b/"], "a\n").await, "b\n");
    // --null-data has two spellings.
    assert_eq!(out(&["--null-data", "s/a/X/g"], "a\0a\0").await, "X\0X\0");
    assert_eq!(
        out(&["--zero-terminated", "s/a/X/g"], "a\0a\0").await,
        "X\0X\0"
    );
}

#[tokio::test]
async fn long_option_separate_and_file() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/f1"), b"a\nb\n")
        .await
        .unwrap();
    fs.write_file(std::path::Path::new("/f2"), b"c\n")
        .await
        .unwrap();
    let sep = run_with(fs.clone(), &["--separate", "-n", "$p", "/f1", "/f2"], None).await;
    assert_eq!(sep.stdout, "b\nc\n");

    fs.write_file(std::path::Path::new("/sc.sed"), b"s/a/X/\n")
        .await
        .unwrap();
    let from_file = run_with(fs, &["--file=/sc.sed"], Some("a\n")).await;
    assert_eq!(from_file.stdout, "X\n");
}

#[tokio::test]
async fn an_empty_script_is_a_passthrough() {
    let result = run_sed(&[""], Some("a\n")).await;
    assert_eq!(
        (result.stdout.to_string(), result.exit_code),
        ("a\n".to_string(), 0)
    );
}

#[tokio::test]
async fn double_dash_ends_option_parsing() {
    // `-e` is then the script, and `-` is not a valid command.
    let result = run_sed(&["--", "-e", "p"], Some("a\n")).await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("unknown command: `-'"),
        "{}",
        result.stderr
    );
}

#[tokio::test]
async fn script_files_are_read_from_the_vfs() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/sc.sed"), b"s/a/X/\ns/X/Y/\n")
        .await
        .unwrap();
    assert_eq!(
        run_with(fs.clone(), &["-f", "/sc.sed"], Some("a\n"))
            .await
            .stdout,
        "Y\n"
    );
    let missing = run_with(fs, &["-f", "/nope.sed"], Some("a\n")).await;
    assert_eq!(missing.exit_code, 4);
    assert!(
        missing.stderr.contains("couldn't open file"),
        "{}",
        missing.stderr
    );
}

#[tokio::test]
async fn unreadable_input_exits_two() {
    let fs = Arc::new(InMemoryFs::new());
    let result = run_with(fs, &["s/a/b/", "/nope.txt"], None).await;
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("can't read"), "{}", result.stderr);
}

#[tokio::test]
async fn unknown_options_are_rejected() {
    let result = run_sed(&["-Q", "s/a/b/"], Some("a\n")).await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("invalid option -- 'Q'"),
        "{}",
        result.stderr
    );
    let long = run_sed(&["--bogus", "p"], Some("a\n")).await;
    assert_eq!(long.exit_code, 1);
    assert!(
        long.stderr.contains("unrecognized option '--bogus'"),
        "{}",
        long.stderr
    );
}

#[tokio::test]
async fn null_data_splits_on_nul() {
    assert_eq!(stdout_of(&["-z", "s/a/X/g"], "a\0a\0").await, "X\0X\0");
}

#[tokio::test]
async fn unresolvable_branch_targets_exit_four() {
    let result = run_sed(&["b nowhere"], Some("a\n")).await;
    assert_eq!(result.exit_code, 4);
    assert!(
        result.stderr.contains("can't find label"),
        "{}",
        result.stderr
    );
}

// TM-INF-022: malformed-regex stderr must not leak `regex` crate Debug shapes.
#[tokio::test]
async fn no_leak_invalid_regex() {
    let r = crate::builtins::debug_leak_check::run(r"echo 1 | sed -E 's/[[:bogus:]]/x/'").await;
    crate::builtins::debug_leak_check::assert_no_leak(
        &r,
        "sed_invalid_regex",
        &["regex::Error", "ParseError {"],
    );
}
