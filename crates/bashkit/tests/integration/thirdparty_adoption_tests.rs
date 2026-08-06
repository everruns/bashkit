//! End-to-end test of the "agent bash tool" adoption shape.
//!
//! This is the integration an embedder writes when it wants bashkit to *be*
//! its shell — a real workspace mounted read-write, and commands bashkit does
//! not implement bridged to host executables so the same code path works on
//! Windows without a real `bash`.
//!
//! Decision: this exists because the shape was only ever validated downstream.
//! The first adopter to write it (crabot) shipped two defects bashkit's own
//! tests could not see: a harness that emptied `PATH`, and a stdin pipe to a
//! host process that never reached EOF and hung the suite. Both are behaviors
//! of *this* composition — `CommandResolver` + `HostMounts` + `realfs` — so
//! they belong here, and this file runs on Windows in CI too.
//!
//! The bridge below is deliberately the whole thing: if bridging a host
//! command needs more code than this, that is a gap in bashkit's API.

#![cfg(feature = "realfs")]

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, CommandResolver, ExecResult, HostMount, HostMounts};
use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Bound on a bridged host command.
///
/// A real bridge needs one regardless; here it also keeps a defect from
/// turning into a hung CI job. A leaked stdin pipe means the child waits for
/// EOF forever, and a test that hangs reports nothing — this turns it into a
/// named failure.
const HOST_TIMEOUT: Duration = Duration::from_secs(20);

/// Run a script through the host shell.
///
/// `sh`/`cmd` rather than a bare binary because the set of standalone
/// executables shared by Linux, macOS, and Windows is effectively empty.
fn shell_out(script: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        ("cmd".into(), vec!["/C".into(), script.into()])
    } else {
        ("sh".into(), vec!["-c".into(), script.into()])
    }
}

/// Map a bridged name to the host program it runs.
///
/// The `host-` prefix matters: bashkit implements `cat`, `echo`, `pwd` and
/// `false` itself, so a test using those names would never reach the bridge —
/// it would silently assert on builtins instead. Returning `None` for
/// unprefixed names also keeps the normal 127 path observable.
fn host_program(name: &str) -> Option<&'static str> {
    Some(match name.strip_prefix("host-")? {
        // Reads stdin to EOF on every platform.
        "cat" => {
            if cfg!(windows) {
                "sort"
            } else {
                "cat"
            }
        }
        "pwd" => {
            if cfg!(windows) {
                "cd"
            } else {
                "pwd"
            }
        }
        "false" => {
            if cfg!(windows) {
                "exit 1"
            } else {
                "false"
            }
        }
        "echo" => "echo",
        _ => return None,
    })
}

/// Bridges one command name to a host process.
///
/// The interesting parts, all of which the first adopter got wrong at least
/// once: the cwd comes from the mount table (never a default), stdin is fed
/// through a pipe that must reach EOF, and the exit code is passed through.
struct HostCommand {
    name: String,
    program: &'static str,
    mounts: Arc<HostMounts>,
}

#[async_trait]
impl Builtin for HostCommand {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // A cwd on no mount is an error. Falling back to some default would
        // run the command in the wrong directory.
        let Some(dir) = self.mounts.resolve(ctx.cwd) else {
            return Ok(ExecResult::err(
                format!("{}: cwd is not on a host mount", self.name),
                1,
            ));
        };

        let script = std::iter::once(self.program.to_string())
            .chain(ctx.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        let (program, args) = shell_out(&script);

        // Bytes, not text: a host command's stdin may not be valid UTF-8.
        let stdin_data = ctx.stdin.map(|s| s.as_bytes().to_vec());
        let output = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(program);
            cmd.args(args)
                .current_dir(dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(if stdin_data.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                });

            let mut child = cmd.spawn()?;
            if let Some(data) = stdin_data {
                use std::io::Write;
                // Dropping the handle closes the pipe. Without that the child
                // waits for EOF forever — the exact hang this file exists for.
                let mut pipe = child.stdin.take().expect("stdin piped");
                pipe.write_all(&data)?;
                drop(pipe);
            }

            let deadline = Instant::now() + HOST_TIMEOUT;
            let status = loop {
                if let Some(status) = child.try_wait()? {
                    break Some(status);
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            };

            // Only drain on a clean exit. After a timeout kill, a grandchild
            // the shell spawned can still hold the write end, and reading
            // would block forever — turning the bounded wait back into a hang.
            //
            // Reading after exit is safe here because the outputs are small.
            // A production bridge must drain concurrently with the wait, or a
            // child that fills the pipe buffer blocks before it can exit.
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if status.is_some() {
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut stdout);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_end(&mut stderr);
                }
            }
            Ok::<_, std::io::Error>((status, stdout, stderr))
        })
        .await
        .expect("spawn_blocking");

        Ok(match output {
            Ok((Some(status), stdout, stderr)) => ExecResult {
                stdout: stdout.into(),
                stderr: stderr.into(),
                exit_code: status.code().unwrap_or(1),
                ..Default::default()
            },
            Ok((None, _, _)) => ExecResult::err(
                format!(
                    "{}: host command exceeded {}s — did stdin reach EOF?",
                    self.name,
                    HOST_TIMEOUT.as_secs()
                ),
                124,
            ),
            // Bash's convention for a command that could not be executed.
            Err(e) => ExecResult::err(format!("{}: {e}", self.name), 127),
        })
    }
}

/// Bridges any name bashkit did not resolve, exactly as an agent tool would.
struct HostBridge {
    mounts: Arc<HostMounts>,
}

impl CommandResolver for HostBridge {
    fn resolve(&self, name: &str) -> Option<Arc<dyn Builtin>> {
        let program = host_program(name)?;
        Some(Arc::new(HostCommand {
            name: name.to_owned(),
            program,
            mounts: Arc::clone(&self.mounts),
        }))
    }
}

struct Fixture {
    bash: Bash,
    workspace: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let workspace = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir(workspace.join("sub")).unwrap();

    // Built up front and shared: the resolver is passed *into* the builder, so
    // it cannot ask the not-yet-existing instance where things are mounted.
    let mounts = Arc::new(HostMounts::new([HostMount {
        host_path: workspace.clone(),
        vfs_path: PathBuf::from("/workspace"),
    }]));

    let bash = Bash::builder()
        .allowed_mount_paths([workspace.clone()])
        .mount_real_readwrite_at(workspace.clone(), "/workspace")
        .cwd("/workspace")
        .command_resolver(Arc::new(HostBridge {
            mounts: Arc::clone(&mounts),
        }))
        .build();

    // The bridge and the real mount must agree, or every cwd mapping is wrong.
    assert_eq!(bash.host_path_for("/workspace"), Some(workspace.clone()));

    Fixture {
        bash,
        workspace,
        _dir: dir,
    }
}

#[tokio::test]
async fn bridged_host_command_runs_and_returns_output() {
    let mut f = fixture();
    let result = f.bash.exec("host-echo hello from host").await.unwrap();
    assert_eq!(result.exit_code, 0, "unexpected: {result:?}");
    assert!(
        result.stdout.contains("hello from host"),
        "unexpected: {result:?}"
    );
}

/// A builtin bashkit implements must win over the bridge — the resolver runs
/// last, so it must never be consulted for `echo`.
#[tokio::test]
async fn builtins_win_over_the_bridge() {
    let mut f = fixture();
    let result = f.bash.exec("echo bridged").await.unwrap();
    assert_eq!(result.stdout, "bridged\n");
}

/// A name the bridge declines still gets the normal `command not found`.
#[tokio::test]
async fn undeclined_names_keep_command_not_found() {
    let mut f = fixture();
    let result = f.bash.exec("definitely-not-a-command-xyz").await.unwrap();
    assert_eq!(result.exit_code, 127, "unexpected: {result:?}");
}

/// Stdin must reach the host process *and* hit EOF. A bridge that leaks the
/// write end hangs here instead of failing, so the timeout is the assertion.
#[tokio::test]
async fn pipeline_stdin_reaches_the_host_process_and_reaches_eof() {
    let mut f = fixture();
    let result = f.bash.exec("echo piped-payload | host-cat").await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "host command did not complete: {result:?}"
    );
    assert!(
        result.stdout.contains("piped-payload"),
        "unexpected: {result:?}"
    );
}

/// `cd` moves the VFS cwd; the bridge must map the *new* cwd to the host, not
/// silently keep spawning in the workspace root.
#[tokio::test]
async fn cd_changes_the_host_spawn_directory() {
    let mut f = fixture();
    let result = f.bash.exec("cd sub && host-pwd").await.unwrap();
    assert_eq!(result.exit_code, 0, "unexpected: {result:?}");
    let printed = result.stdout.trim().replace('\\', "/");
    assert!(
        printed.ends_with("/sub"),
        "host command ran in the wrong directory: {printed}"
    );
}

/// Writes through bashkit's own builtins must land on the real filesystem —
/// the reason the workspace is mounted read-write rather than overlaid.
#[tokio::test]
async fn builtin_writes_reach_the_real_filesystem() {
    let mut f = fixture();
    let result = f
        .bash
        .exec("echo written > out.txt && mkdir -p sub2 && cp out.txt sub2/copy.txt")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "unexpected: {result:?}");

    assert_eq!(
        std::fs::read_to_string(f.workspace.join("out.txt")).unwrap(),
        "written\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.workspace.join("sub2/copy.txt")).unwrap(),
        "written\n"
    );
}

/// A non-zero host exit code behaves like any other command: it does not abort
/// the script, and it drives `&&` / `||`.
#[tokio::test]
async fn host_exit_codes_drive_control_flow() {
    let mut f = fixture();
    let result = f.bash.exec("host-false || echo recovered").await.unwrap();
    assert!(
        result.stdout.contains("recovered"),
        "unexpected: {result:?}"
    );

    let result = f.bash.exec("host-false; echo after").await.unwrap();
    assert!(
        result.stdout.contains("after"),
        "script aborted: {result:?}"
    );

    let result = f.bash.exec("host-false").await.unwrap();
    assert_ne!(result.exit_code, 0);
}

/// The whole point of the composition: syntax bashkit implements (heredocs,
/// command substitution, pipelines) works with bridged commands mixed in,
/// with no real `bash` process anywhere.
#[tokio::test]
async fn bashkit_syntax_composes_with_bridged_commands() {
    let mut f = fixture();
    let result = f
        .bash
        .exec("cat <<'EOF'\nheredoc line\nEOF\necho \"host says: $(host-echo ok)\"")
        .await
        .unwrap();
    assert!(
        result.stdout.contains("heredoc line"),
        "unexpected: {result:?}"
    );
    assert!(
        result.stdout.contains("host says: ok"),
        "unexpected: {result:?}"
    );
}
