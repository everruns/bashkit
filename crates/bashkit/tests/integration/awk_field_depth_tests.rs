//! THREAT[TM-DOS-027]: field-operand recursion must share AWK depth accounting.
//! Run potentially aborting programs only in a supervised child test process
//! (see `bounded_child` for the bounds and why memory is RSS-capped).

use std::path::Path;
use std::process::Command;

use super::bounded_child;

const CHILD: &str = "BASHKIT_AWK_FIELD_DEPTH_CHILD";
const FINITE_PROGRAM: &str = "BEGIN { $0=\"a b\"; i=0; print $++i, $!0, $+2, $-0 }";

#[test]
#[cfg(unix)]
fn awk_field_depth_bounded() {
    bounded_child::run(
        "awk-depth",
        CHILD,
        "awk_field_depth_tests::awk_field_depth_child",
    );
}

#[test]
fn awk_field_depth_child() {
    if !bounded_child::is_child(CHILD) {
        return;
    }
    // Keep the production API on a typical 2 MiB embedding thread stack.
    bounded_child::on_small_stack(|| async {
        let mut bash = bashkit::Bash::new();
        for token in ["!", "- ", "+ ", "! + - "] {
            for prefix in ["$", "++$", "getline $", "$++$"] {
                let program = format!("BEGIN {{ print {prefix}{}1 }}", token.repeat(200));
                for file in [false, true] {
                    let command = if file {
                        bash.fs()
                            .write_file(Path::new("/tmp/payload.awk"), program.as_bytes())
                            .await
                            .unwrap();
                        "awk -f /tmp/payload.awk".to_owned()
                    } else {
                        format!("awk '{program}'")
                    };
                    let result = bash.exec(&command).await.unwrap();
                    assert_eq!(
                        result.exit_code, 2,
                        "token={token}, prefix={prefix}, file={file}"
                    );
                    assert!(result.stdout.is_empty());
                    assert!(
                        result.stderr.contains("nesting too deep"),
                        "{}",
                        result.stderr
                    );
                    assert!(result.stderr.len() <= 1024);
                    let recovered = bash.exec("awk 'BEGIN { print 42 }'").await.unwrap();
                    assert_eq!(recovered.exit_code, 0);
                    assert_eq!(recovered.stdout, "42\n");
                    assert!(recovered.stderr.is_empty());
                }
            }
        }
        // BEGIN block + print + expression + field consume four levels.
        for (count, status) in [(95, 0), (96, 0), (97, 2)] {
            for token in ["! ", "- ", "+ "] {
                let program = format!(
                    "BEGIN {{ $0=\"one two\"; print ${}0 }}",
                    token.repeat(count)
                );
                let result = bash.exec(&format!("awk '{program}'")).await.unwrap();
                assert_eq!(
                    result.exit_code, status,
                    "count={count}, token={token}: {}",
                    result.stderr
                );
                if status == 0 {
                    assert!(result.stderr.is_empty());
                    let expected = if token == "! " && count % 2 == 1 {
                        "one\n"
                    } else {
                        "one two\n"
                    };
                    assert_eq!(result.stdout, expected);
                } else {
                    assert!(result.stderr.contains("101 levels, max 100"));
                }
            }
        }
        // Malformed operands remain syntax errors; recovery works afterwards.
        let bad = bash.exec("awk 'BEGIN { print $! }'").await.unwrap();
        assert_eq!(bad.exit_code, 1);
        assert!(bad.stderr.contains("syntax error"));
        let good = bash.exec(&format!("awk '{FINITE_PROGRAM}'")).await.unwrap();
        assert_eq!(good.exit_code, 0);
        assert_eq!(good.stdout, "a a b a b\n");
        assert!(good.stderr.is_empty());
    });
}

#[tokio::test]
async fn finite_field_operands_match_gawk() {
    let oracle = match Command::new("gawk").arg(FINITE_PROGRAM).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("run gawk oracle: {error}"),
    };
    assert!(oracle.status.success());

    let mut bash = bashkit::Bash::new();
    let result = bash.exec(&format!("awk '{FINITE_PROGRAM}'")).await.unwrap();
    assert_eq!(result.exit_code, oracle.status.code().unwrap());
    assert_eq!(result.stdout.as_bytes(), oracle.stdout);
    assert_eq!(result.stderr.as_bytes(), oracle.stderr);
}
