//! THREAT[TM-DOS-027]: field-operand recursion must share AWK depth accounting.
//! Run potentially aborting programs only in a supervised child test process.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CHILD: &str = "BASHKIT_AWK_FIELD_DEPTH_CHILD";
const FINITE_PROGRAM: &str = "BEGIN { $0=\"a b\"; i=0; print $++i, $!0, $+2, $-0 }";

fn drain(
    mut stream: impl Read + Send + 'static,
    exceeded: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let n = stream.read(&mut chunk).expect("read child output");
            if n == 0 {
                return output;
            }
            if output.len() + n > 65536 {
                exceeded.store(true, Ordering::Relaxed);
            } else {
                output.extend_from_slice(&chunk[..n]);
            }
        }
    })
}

#[test]
#[cfg(unix)]
fn awk_field_depth_bounded() {
    // OS CPU/stack/core limits, Linux address-space cap, continuous output
    // drains and a wall watchdog also contain a regression that aborts/hangs.
    #[cfg(target_os = "linux")]
    let limits =
        "ulimit -t 10 && ulimit -s 8192 && ulimit -c 0 && ulimit -v 2097152 && exec \"$@\"";
    #[cfg(not(target_os = "linux"))]
    let limits = "ulimit -t 10 && ulimit -s 8192 && ulimit -c 0 && exec \"$@\"";
    let mut command = Command::new("/bin/sh");
    command.args(["-c", limits, "awk-depth"]);
    let mut child = command
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "awk_field_depth_tests::awk_field_depth_child",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bounded child");
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = drain(child.stdout.take().unwrap(), exceeded.clone());
    let stderr = drain(child.stderr.take().unwrap(), exceeded.clone());
    let start = Instant::now();
    let mut stopped = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if exceeded.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(20) {
            stopped = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout.join().unwrap();
    let stderr = stderr.join().unwrap();
    assert!(
        !stopped && !exceeded.load(Ordering::Relaxed),
        "child exceeded bounds"
    );
    assert!(
        status.success(),
        "child status {status}: {}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(
        String::from_utf8_lossy(&stdout).contains("1 passed"),
        "child test did not run"
    );
}

#[test]
fn awk_field_depth_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // Keep the production API on a typical 2 MiB embedding thread stack.
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut bash = bashkit::Bash::new();
                    for token in ["!", "- ", "+ ", "! + - "] {
                        for prefix in ["$", "++$", "getline $", "$++$"] {
                            let program =
                                format!("BEGIN {{ print {prefix}{}1 }}", token.repeat(200));
                            for file in [false, true] {
                                let command = if file {
                                    bash.fs()
                                        .write_file(
                                            Path::new("/tmp/payload.awk"),
                                            program.as_bytes(),
                                        )
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
                                let recovered =
                                    bash.exec("awk 'BEGIN { print 42 }'").await.unwrap();
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
        })
        .unwrap()
        .join()
        .unwrap();
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
