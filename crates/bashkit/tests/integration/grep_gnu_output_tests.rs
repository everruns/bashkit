//! `grep` against GNU grep 3.11 output, case by case: prefix order,
//! context groups, option parsing, POSIX regex dialects, binary input.
//!
//! Expected values are GNU grep 3.11 (Debian, C.UTF-8) outputs recorded by
//! an external oracle bench; see the grep section of
//! `knowledge/foundations/builtins.md`.

use bashkit::Bash;

struct Out {
    stdout: Vec<u8>,
    stderr: String,
    code: i32,
}

async fn run(script: &str) -> Out {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec(script)
        .await
        .unwrap_or_else(|e| panic!("{script:?} failed: {e}"));
    Out {
        stdout: r.stdout.as_bytes().to_vec(),
        stderr: r.stderr.to_string(),
        code: r.exit_code,
    }
}

async fn stdout(script: &str) -> String {
    String::from_utf8_lossy(&run(script).await.stdout).into_owned()
}

const YESNO: &str = "printf '[A01 no ]\\n[B02 no ]\\n[C03 yes]\\n[D04 yes]\\n[E05 yes]\\n[F06 no ]\\n[G07 no ]\\n[H08 yes]\\n'";

#[tokio::test]
async fn line_number_comes_before_byte_offset() {
    assert_eq!(
        stdout(&format!("{YESNO} | grep -n -b yes | head -2")).await,
        "3:20:[C03 yes]\n4:30:[D04 yes]\n"
    );
    // Context lines use `-` after every prefix.
    assert_eq!(
        stdout(&format!("{YESNO} | grep -nb -B1 C03")).await,
        "2-10-[B02 no ]\n3:20:[C03 yes]\n"
    );
}

#[tokio::test]
async fn context_groups_get_separators() {
    let s = "printf 'x1\\n2\\n3\\n4\\nx5\\n' | grep";
    assert_eq!(stdout(&format!("{s} -A1 x")).await, "x1\n2\n--\nx5\n");
    assert_eq!(stdout(&format!("{s} -1 x")).await, "x1\n2\n--\n4\nx5\n");
    assert_eq!(
        stdout(&format!("{s} -A1 --group-separator=XYZ x")).await,
        "x1\n2\nXYZ\nx5\n"
    );
    // getopt_long: a unique prefix names the option.
    assert_eq!(stdout(&format!("{s} -A1 --no-gr x")).await, "x1\n2\nx5\n");
    // -A0 still separates non-adjacent matches.
    assert_eq!(stdout(&format!("{s} -A0 x")).await, "x1\n--\nx5\n");
    // No context option: no separators.
    assert_eq!(stdout(&format!("{s} x")).await, "x1\nx5\n");
}

#[tokio::test]
async fn separator_between_files() {
    let r =
        stdout("printf 'x1\\n2\\n' > a.txt; printf 'x1\\n' > b.txt; grep -A1 x a.txt b.txt").await;
    assert_eq!(r, "a.txt:x1\na.txt-2\n--\nb.txt:x1\n");
}

#[tokio::test]
async fn trailing_context_after_max_count_is_unconditional() {
    assert_eq!(
        stdout("printf 'x1\\n2\\nx3\\n4\\n' | grep -m1 -A2 x").await,
        "x1\n2\nx3\n"
    );
}

#[tokio::test]
async fn only_matching_follows_gnu() {
    // Leftmost-longest, across alternatives and across -e patterns.
    assert_eq!(stdout("echo abcd | grep -oE 'ab|abcd'").await, "abcd\n");
    assert_eq!(stdout("echo abc | grep -o -e ab -e abc").await, "abc\n");
    // Empty matches are skipped.
    assert_eq!(
        stdout("printf 'a1b22\\nxyz\\n' | grep -o '[0-9]*'").await,
        "1\n22\n"
    );
    // -b gives each match's own offset; -n comes first.
    assert_eq!(
        stdout("printf 'XfooYfoo\\nz\\nfoo\\n' | grep -onb foo").await,
        "1:1:foo\n1:5:foo\n3:11:foo\n"
    );
    // -v -o prints nothing for selected lines but succeeds.
    let r = run("printf 'yes\\nno\\n' | grep -v -o yes").await;
    assert_eq!((r.stdout.as_slice(), r.code), (&b""[..], 0));
}

#[tokio::test]
async fn back_references_in_bre_and_ere() {
    assert_eq!(
        stdout("printf 'abab\\nabba\\nxababx\\n' | grep '\\(ab\\)\\1'").await,
        "abab\nxababx\n"
    );
    assert_eq!(
        stdout("printf 'hello\\nworld\\n' | grep -oE '(.)\\1'").await,
        "ll\n"
    );
    let r = run("echo a | grep '\\(a\\)\\2'").await;
    assert_eq!(r.code, 2);
    assert_eq!(r.stderr, "grep: Invalid back reference\n");
}

#[tokio::test]
async fn bre_literals_and_ere_leading_operators() {
    assert_eq!(stdout("echo 'a+b?c*d' | grep -o 'a+b'").await, "a+b\n");
    assert_eq!(stdout("echo 'a^b' | grep 'a^b'").await, "a^b\n");
    assert_eq!(stdout("echo '*d' | grep -o '*d'").await, "*d\n");
    let r = run("echo xyz | grep -E '*xyz'").await;
    assert_eq!(r.stdout, b"xyz\n");
    assert_eq!(r.stderr, "grep: warning: * at start of expression\n");
    assert_eq!(stdout("echo 'a{x}' | grep -E 'a{x}'").await, "a{x}\n");
    assert_eq!(stdout("echo 'a)' | grep -E 'a)'").await, "a)\n");
}

#[tokio::test]
async fn syntax_errors_use_gnu_messages() {
    for (pat, flags, msg) in [
        ("(ab", "-E", "Unmatched ( or \\("),
        ("\\(ab", "-G", "Unmatched ( or \\("),
        ("[abc", "-E", "Unmatched [, [^, [:, [., or [="),
        ("[z-a]", "-E", "Invalid range end"),
        ("[[:foo:]]", "-E", "Invalid character class name"),
        ("a{3,1}", "-E", "Invalid content of \\{\\}"),
        ("ab\\", "-G", "Trailing backslash"),
        (
            "[:space:]",
            "-E",
            "character class syntax is [[:space:]], not [:space:]",
        ),
    ] {
        let r = run(&format!("echo a | grep {flags} -e '{pat}'")).await;
        assert_eq!(r.code, 2, "{pat}");
        assert_eq!(r.stderr, format!("grep: {msg}\n"), "{pat}");
    }
}

#[tokio::test]
async fn option_parsing_matches_getopt_long() {
    let r = run("echo a | grep --frobnicate a").await;
    assert_eq!(r.code, 2);
    assert_eq!(
        r.stderr,
        "grep: unrecognized option '--frobnicate'\nUsage: grep [OPTION]... PATTERNS [FILE]...\nTry 'grep --help' for more information.\n"
    );
    let r = run("echo a | grep --no a").await;
    assert_eq!(r.code, 2);
    assert!(r.stderr.starts_with("grep: option '--no' is ambiguous;"));
    assert_eq!(stdout("printf 'ax\\nb\\n' | grep -nT x").await, "1:\tax\n");
    assert_eq!(
        stdout("printf 'a\\nb\\n' | grep -H --label=input b").await,
        "input:b\n"
    );
    assert_eq!(
        stdout("printf 'a\\nb\\nab\\n' | grep -n b - ").await,
        "2:b\n3:ab\n"
    );
}

#[tokio::test]
async fn empty_pattern_file_matches_nothing() {
    let r = run("printf '' > p; printf 'alpha\\n' | grep -f p").await;
    assert_eq!((r.stdout.as_slice(), r.code), (&b""[..], 1));
    assert_eq!(
        stdout("printf '' > p; printf 'alpha\\n' | grep -v -f p").await,
        "alpha\n"
    );
}

#[tokio::test]
async fn directories_option() {
    let setup = "mkdir -p src; echo x > src/a.txt; echo 'x here' > f.txt;";
    let r = run(&format!("{setup} grep x src f.txt")).await;
    assert_eq!(r.code, 2);
    assert_eq!(r.stdout, b"f.txt:x here\n");
    assert_eq!(r.stderr, "grep: src: Is a directory\n");
    let r = run(&format!("{setup} grep -d skip x src f.txt")).await;
    assert_eq!((r.stdout.as_slice(), r.code), (&b"f.txt:x here\n"[..], 0));
    assert_eq!(
        stdout(&format!("{setup} grep -d recurse x src")).await,
        "src/a.txt:x\n"
    );
}

#[tokio::test]
async fn recursive_symlinks_only_followed_by_capital_r() {
    let setup =
        "mkdir d; echo 'secret a' > d/a.txt; echo 'secret out' > o.txt; ln -s ../o.txt d/link.txt;";
    assert_eq!(
        stdout(&format!("{setup} grep -r secret d")).await,
        "d/a.txt:secret a\n"
    );
    assert_eq!(
        stdout(&format!("{setup} grep -R secret d")).await,
        "d/a.txt:secret a\nd/link.txt:secret out\n"
    );
}

#[tokio::test]
async fn invalid_utf8_is_binary_unless_text() {
    let r = run("printf 'caf\\351\\n' > l.txt; grep caf l.txt").await;
    assert_eq!(r.stdout, b"");
    assert_eq!(r.stderr, "grep: l.txt: binary file matches\n");
    assert_eq!(r.code, 0);
    // Lines before the first undecodable one are still printed.
    let r = run("printf 'cafe\\ncaf\\351\\n' > l.txt; grep caf l.txt").await;
    assert_eq!(r.stdout, b"cafe\n");
    assert_eq!(r.stderr, "grep: l.txt: binary file matches\n");
    // -a passes the bytes through unchanged.
    let r = run("printf 'caf\\351\\n' > l.txt; grep -a caf l.txt").await;
    assert_eq!(r.stdout, b"caf\xe9\n");
}

#[tokio::test]
async fn carriage_return_is_line_data() {
    assert_eq!(
        stdout("printf 'ax\\r\\nbx\\nc\\r\\n' | grep -c 'x$'").await,
        "1\n"
    );
}

#[tokio::test]
async fn unicode_classes_and_icase_classes() {
    assert_eq!(
        stdout("echo 'héllo École' | grep -oE '[[:alpha:]]+'").await,
        "héllo\nÉcole\n"
    );
    assert_eq!(
        stdout("echo 'MiXeD' | grep -oiE '[[:upper:]]+'").await,
        "MiXeD\n"
    );
    let r = run("echo é | grep -E '[à-ú]'").await;
    assert_eq!(r.code, 2);
    assert_eq!(r.stderr, "grep: Invalid collation character\n");
}
