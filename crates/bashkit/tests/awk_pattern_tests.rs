//! Tests for awk compound patterns and related fixes

use bashkit::Bash;
use std::path::Path;

/// Issue #808: awk compound pattern `expr && /regex/` should match correctly
#[tokio::test]
async fn awk_compound_pattern_and_regex() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(Path::new("/tmp/t.txt"), b"id: t1\nstatus: open\n")
        .await
        .unwrap();
    let result = bash
        .exec(
            r#"awk '
BEGIN { FS=": "; flag=1 }
flag && /^id:/ { print "id matched: " $0 }
flag && /^status:/ { print "status matched: " $0 }
' /tmp/t.txt"#,
        )
        .await
        .unwrap();
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "id matched: id: t1");
    assert_eq!(lines[1], "status matched: status: open");
}
