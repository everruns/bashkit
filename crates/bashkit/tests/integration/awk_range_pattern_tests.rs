//! Tests for awk range patterns (/start/,/end/)

use bashkit::Bash;
use std::path::Path;

#[tokio::test]
async fn awk_range_pattern_basic() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"printf "a\nb\nc\nd\ne\n" | awk '/b/,/d/{print}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "b\nc\nd\n");
}

#[tokio::test]
async fn awk_range_pattern_with_exclusion() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/tmp/range_test.txt"),
        b"before\n<!-- text begin -->\ncontent line 1\ncontent line 2\n<!-- text end -->\nafter\n",
    )
    .await
    .unwrap();

    let result = bash
        .exec(
            r#"awk '/<!-- text begin -->/,/<!-- text end -->/{if (!/<!-- text begin -->/ && !/<!-- text end -->/) print}' /tmp/range_test.txt"#,
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "content line 1\ncontent line 2\n");
}

#[tokio::test]
async fn awk_range_pattern_multiple_ranges() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"printf "a\nSTART\nb\nEND\nc\nSTART\nd\nEND\ne\n" | awk '/START/,/END/{print}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "START\nb\nEND\nSTART\nd\nEND\n");
}

#[tokio::test]
async fn awk_range_pattern_with_action_block() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"printf "1\n2\n3\n4\n5\n" | awk '/2/,/4/{print "-> " $0}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "-> 2\n-> 3\n-> 4\n");
}

/// bashblog get_post_title pattern
#[tokio::test]
async fn awk_range_pattern_html_title_extraction() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/tmp/post.html"),
        b"<h3><a class=\"ablack\" href=\"test.html\">\nMy Post Title\n</a></h3>\n",
    )
    .await
    .unwrap();

    let result = bash
        .exec(
            r#"awk '/<h3><a class="ablack" href=".+">/, /<\/a><\/h3>/{if (!/<h3><a class="ablack" href=".+">/ && !/<\/a><\/h3>/) print}' /tmp/post.html"#,
        )
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "My Post Title");
}

/// Range that starts but never ends (should match all remaining lines)
#[tokio::test]
async fn awk_range_pattern_unterminated() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"printf "a\nSTART\nb\nc\n" | awk '/START/,/END/{print}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "START\nb\nc\n");
}

/// Range with default print action (no action block)
#[tokio::test]
async fn awk_range_pattern_default_action() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"printf "x\na\nb\nc\ny\n" | awk '/a/,/c/'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "a\nb\nc\n");
}

/// Start/end matching same record should only match that single record.
#[tokio::test]
async fn awk_range_pattern_start_end_same_line() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"printf "a\nb\na\nc\n" | awk '/a/,/a/{print}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "a\na\n");
}
