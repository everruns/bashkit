//! Live command I/O must make progress before producers finish.

use async_trait::async_trait;
use bashkit::{Bash, Command, CommandContext, CommandStatus};
use std::sync::Arc;
use tokio::sync::Notify;

struct Copy;

#[tokio::test]
async fn input_process_substitution_captures_streaming_producers() {
    for script in [
        "paste <(seq 3) <(seq 4 6)",
        "paste <(seq 3 | copy) <(seq 4 6 | copy)",
        "paste <(seq 3) <(seq 4 6) | copy",
    ] {
        let mut bash = Bash::builder().command("copy", Copy).build();
        let result = bash.execute(script).capture(1024).await.unwrap();
        assert_eq!(result.exit_code, 0, "{script}");
        assert_eq!(result.stdout.as_bytes(), b"1\t4\n2\t5\n3\t6\n", "{script}");
        assert!(result.stderr.is_empty(), "{script}");
    }
}

struct MixedOutput;
#[async_trait]
impl Command for MixedOutput {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        ctx.stdout.write_all(b"a").await?;
        ctx.stderr.write_all(b"b").await?;
        ctx.stdout.write_all(b"c").await?;
        Ok(CommandStatus::success())
    }
}

#[tokio::test]
async fn command_redirects_preserve_write_and_duplication_order() {
    let mut bash = Bash::builder().command("mixed", MixedOutput).build();
    let result = bash.execute("mixed 2>&1").capture(64).await.unwrap();
    assert_eq!(result.stdout.as_bytes(), b"abc");
    assert!(result.stderr.is_empty());
    let result = bash
        .execute("mixed >/out 2>&1; cat /out")
        .capture(64)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b"abc");
    let result = bash
        .execute("mixed 2>&1 >/out; cat /out")
        .capture(64)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b"bac");
}

#[tokio::test]
async fn redirected_command_writes_are_visible_before_it_completes() {
    let release = Arc::new(Notify::new());
    let mut bash = Bash::builder()
        .command(
            "produce",
            GatedProducer {
                release: release.clone(),
            },
        )
        .build();
    let fs = bash.fs();
    let execution = async { bash.execute("produce >/out").capture(64).await };
    let observer = async {
        loop {
            if fs
                .read_file(std::path::Path::new("/out"))
                .await
                .is_ok_and(|b| b == b"error: first\n")
            {
                release.notify_one();
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(execution, observer)
    })
    .await
    .unwrap();
    assert!(result.unwrap().stdout.is_empty());
    assert_eq!(
        fs.read_file(std::path::Path::new("/out")).await.unwrap(),
        b"error: first\nerror: last\n"
    );
}

struct BinaryProducer {
    handle_broken_pipe: bool,
}

#[async_trait]
impl Command for BinaryProducer {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        let bytes: Vec<u8> = (0..1_100_000).map(|n| (n % 256) as u8).collect();
        match ctx.stdout.write_all(&bytes).await {
            Err(bashkit::Error::Io(e))
                if self.handle_broken_pipe && e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                Ok(CommandStatus::with_code(23))
            }
            result => {
                result?;
                Ok(CommandStatus::success())
            }
        }
    }
}

#[tokio::test]
async fn more_than_one_megabyte_crosses_command_and_builtin_stages_unchanged() {
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::default().max_stdout_bytes(1_200_000))
        .command(
            "binary",
            BinaryProducer {
                handle_broken_pipe: false,
            },
        )
        .command("copy", Copy)
        .build();
    let result = bash
        .execute("binary | copy | cat")
        .capture(1_200_000)
        .await
        .unwrap();
    assert_eq!(result.stdout.len(), 1_100_000);
    assert!(
        result
            .stdout
            .as_bytes()
            .iter()
            .enumerate()
            .all(|(n, &b)| b == (n % 256) as u8)
    );
    assert!(!result.stdout_truncated);
}

#[tokio::test]
async fn handled_broken_pipe_keeps_handler_status() {
    let mut bash = Bash::builder()
        .command(
            "binary",
            BinaryProducer {
                handle_broken_pipe: true,
            },
        )
        .build();
    let result = bash
        .execute("binary | head -c1; printf ':%s' \"${PIPESTATUS[*]}\"")
        .capture(64)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b"\0:23 0");
}

#[async_trait]
impl Command for Copy {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        while let Some(bytes) = ctx.stdin.read_chunk().await? {
            ctx.stdout.write_all(&bytes).await?;
        }
        Ok(CommandStatus::success())
    }
}

struct GatedProducer {
    release: Arc<Notify>,
}

#[async_trait]
impl Command for GatedProducer {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        ctx.stdout.write_all(b"error: first\n").await?;
        self.release.notified().await;
        ctx.stdout.write_all(b"error: last\n").await?;
        Ok(CommandStatus::success())
    }
}

#[tokio::test]
async fn final_filter_delivers_before_producer_finishes() {
    let release = Arc::new(Notify::new());
    let mut bash = Bash::builder()
        .command(
            "produce",
            GatedProducer {
                release: release.clone(),
            },
        )
        .command("copy", Copy)
        .build();
    let (mut writer, mut reader) = tokio::io::duplex(64);
    let execution = async {
        bash.execute("produce | copy | grep error")
            .stdout(&mut writer)
            .capture(1024)
            .await
    };
    let observer = async {
        use tokio::io::AsyncReadExt;
        let mut first = [0; 13];
        reader.read_exact(&mut first).await.unwrap();
        assert_eq!(&first, b"error: first\n");
        release.notify_one();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(execution, observer)
    })
    .await
    .expect("final filter buffered until producer completion");
    let result = result.unwrap();
    assert_eq!(result.stdout.as_bytes(), b"error: first\nerror: last\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn streaming_capture_and_command_substitution_are_distinct() {
    let mut bash = Bash::builder().command("copy", Copy).build();
    let result = bash
        .execute("v=$(printf hello | copy); printf '%s' \"$v\" | copy")
        .capture(32)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b"hello");
    assert!(!result.stdout_truncated);
    let result = bash
        .execute("printf 'abcdefgh' | copy")
        .capture(3)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b"abc");
    assert!(result.stdout_truncated);
    let result = bash.execute("printf hello | copy").await.unwrap();
    assert!(result.stdout.is_empty());
}

#[tokio::test]
async fn command_redirection_keeps_output_out_of_caller_sink() {
    let mut bash = Bash::builder().command("copy", Copy).build();
    let result = bash
        .execute("printf hello | copy > /out; cat /out")
        .capture(32)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b"hello");
}

struct StalledWriter;

impl tokio::io::AsyncWrite for StalledWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Pending
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn deadline_interrupts_a_stalled_sink_and_session_recovers() {
    let limits = bashkit::ExecutionLimits::default().timeout(std::time::Duration::from_millis(50));
    let mut bash = Bash::builder().limits(limits).build();
    let mut sink = StalledWriter;
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        bash.execute("echo hello").stdout(&mut sink).await
    })
    .await
    .expect("sink was not subject to execution deadline");
    assert!(matches!(
        result,
        Err(bashkit::Error::ResourceLimit(
            bashkit::LimitExceeded::Timeout(_)
        ))
    ));
    assert_eq!(
        bash.execute("echo recovered")
            .capture(64)
            .await
            .unwrap()
            .stdout
            .as_bytes(),
        b"recovered\n"
    );
}

#[tokio::test]
async fn failed_sink_drops_pending_producer_and_revokes_handles() {
    struct KeepHandles(Arc<std::sync::Mutex<Option<bashkit::CommandOutput>>>);
    #[async_trait]
    impl Command for KeepHandles {
        async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
            *self.0.lock().unwrap() = Some(ctx.stdout.clone());
            ctx.stdout.write_all(b"first").await?;
            std::future::pending().await
        }
    }
    let handle = Arc::new(std::sync::Mutex::new(None));
    let mut bash = Bash::builder()
        .command("produce", KeepHandles(handle.clone()))
        .build();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        bash.execute("produce")
            .stdout_fn(|_| async { Err(std::io::Error::other("sink closed").into()) })
            .await
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(bashkit::Error::Io(_))));
    let output = handle.lock().unwrap().take().unwrap();
    assert!(output.write_all(b"late").await.is_err());
    assert_eq!(
        bash.execute("echo recovered")
            .capture(64)
            .await
            .unwrap()
            .stdout
            .as_bytes(),
        b"recovered\n"
    );
}

#[tokio::test]
async fn cancellation_interrupts_pending_command_and_sink() {
    for script in ["produce", "echo first"] {
        let mut bash = Bash::builder()
            .command(
                "produce",
                GatedProducer {
                    release: Arc::new(Notify::new()),
                },
            )
            .build();
        let cancel = bash.cancellation_token();
        let mut writer = StalledWriter;
        let execution = async { bash.execute(script).stdout(&mut writer).await };
        let cancellation = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(execution, cancellation)
        })
        .await
        .unwrap();
        assert!(matches!(result, Err(bashkit::Error::Cancelled)));
        bash.cancellation_token()
            .store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            bash.execute("echo recovered")
                .capture(64)
                .await
                .unwrap()
                .stdout
                .as_bytes(),
            b"recovered\n"
        );
    }
}

#[tokio::test]
async fn redirecting_stdout_keeps_stderr_live() {
    struct ErrorProducer(Arc<Notify>);
    #[async_trait]
    impl Command for ErrorProducer {
        async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
            ctx.stdout.write_all(b"file").await?;
            ctx.stderr.write_all(b"first").await?;
            self.0.notified().await;
            Ok(CommandStatus::success())
        }
    }
    let release = Arc::new(Notify::new());
    let mut bash = Bash::builder()
        .command("produce", ErrorProducer(release.clone()))
        .build();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        bash.execute("produce >/out")
            .capture(64)
            .stderr_fn(move |bytes| {
                assert_eq!(bytes, b"first");
                release.notify_one();
                async { Ok(()) }
            })
            .await
    })
    .await
    .expect("stderr was buffered behind an unrelated stdout redirect")
    .unwrap();
    assert!(result.stdout.is_empty());
    assert_eq!(result.stderr.as_bytes(), b"first");
}

#[tokio::test]
async fn sandbox_output_caps_apply_to_binary_sinks_independently_of_capture() {
    let mut bash = Bash::builder()
        .limits(
            bashkit::ExecutionLimits::default()
                .max_stdout_bytes(3)
                .max_stderr_bytes(2),
        )
        .command("copy", Copy)
        .build();
    let bytes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = bytes.clone();
    let result = bash
        .execute("printf '\\000\\377abcdef' | copy; printf 'error' >&2")
        .capture(1)
        .stdout_fn(move |chunk| {
            sink.lock().unwrap().extend(chunk);
            async { Ok(()) }
        })
        .await
        .unwrap();
    assert_eq!(*bytes.lock().unwrap(), b"\0\xffa");
    assert_eq!(result.stdout.as_bytes(), b"\0");
    assert_eq!(result.stderr.as_bytes(), b"e");
    assert!(result.stdout_truncated && result.stderr_truncated);
    let result = bash.execute("printf 'abcdef'").await.unwrap();
    assert!(result.stdout.is_empty());
    assert!(result.stdout_truncated);
}

#[tokio::test]
async fn substitution_in_a_streaming_producer_does_not_leak_to_its_pipe() {
    let script = "for i in $(seq 1 20); do echo item-$i; done | grep -E 'item-[0-9]$' | sort -t- -k2 -n | wc -l";
    let mut bash = Bash::new();
    assert_eq!(bash.exec(script).await.unwrap().stdout.as_bytes(), b"9\n");
    assert_eq!(
        bash.execute(script)
            .capture(64)
            .await
            .unwrap()
            .stdout
            .as_bytes(),
        b"9\n"
    );
    let mut bash = Bash::builder().command("copy", Copy).build();
    assert_eq!(
        bash.execute("{ v=$(printf hello | copy); printf '%s' \"$v\"; } | cat")
            .capture(64)
            .await
            .unwrap()
            .stdout
            .as_bytes(),
        b"hello"
    );
}

#[tokio::test]
async fn accepted_output_is_drained_before_a_command_limit_error() {
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::default().max_commands(2))
        .build();
    let bytes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = bytes.clone();
    let result = bash
        .execute("echo one; echo two; echo three")
        .stdout_fn(move |chunk| {
            sink.lock().unwrap().extend(chunk);
            async { Ok(()) }
        })
        .await;
    assert!(result.is_err());
    assert!(bytes.lock().unwrap().starts_with(b"one\n"));
}

#[tokio::test]
async fn closed_diagnostic_fd_keeps_a_command_io_failure_an_ordinary_status() {
    let mut bash = Bash::builder().command("mixed", MixedOutput).build();
    let result = bash
        .execute("mixed >&- 2>&-; printf ':done'")
        .capture(64)
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), b":done");
    assert!(result.stderr.is_empty());
    assert_eq!(result.exit_code, 0);
}
