//! Regression test for #276: parse_int doesn't trim whitespace

use bashkit::Bash;

#[tokio::test]
async fn issue_276_parse_int_trims_whitespace() {
    let mut bash = Bash::new();
    // wc pads output with spaces; integer comparison must still work
    let r = bash
        .exec(r#"[ "  3  " -ge 2 ] && echo yes || echo no"#)
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "yes");
}

#[tokio::test]
async fn issue_276_wc_output_in_comparison() {
    let mut bash = Bash::new();
    let r = bash
        .exec(r#"count=$(echo -e "a\nb\nc" | wc -l); [ "$count" -ge 2 ] && echo has || echo no"#)
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "has");
}
