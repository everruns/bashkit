//! Tiny ordinary dd copy contracts; admission arithmetic lives in dd unit tests.

use bashkit::{Bash, ExecOptions};

fn is_gnu_dd_version(stdout: &[u8]) -> bool {
    // GNU dd prints PACKAGE (`coreutils`), not PACKAGE_NAME (`GNU coreutils`).
    stdout.starts_with(b"dd (coreutils) ") || stdout.starts_with(b"dd (GNU coreutils) ")
}

#[test]
fn dd_oracle_recognizes_gnu_package_banner() {
    assert!(is_gnu_dd_version(
        b"dd (coreutils) 9.4\nCopyright (C) 2023 Free Software Foundation, Inc.\n"
    ));
    assert!(is_gnu_dd_version(b"dd (GNU coreutils) 9.12\n"));
    assert!(!is_gnu_dd_version(b"dd (uutils coreutils) 0.12.0\n"));
    assert!(!is_gnu_dd_version(b"dd (BSD)\n"));
    assert!(!is_gnu_dd_version(b"other program GNU coreutils\n"));
}

#[tokio::test]
async fn dd_normal_copies_match_gnu() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Some(program) = ["gdd", "dd"].into_iter().find(|program| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success() && is_gnu_dd_version(&out.stdout))
    }) else {
        assert!(
            !(cfg!(target_os = "linux") && std::env::var_os("CI").is_some()),
            "GNU dd is required for Linux CI"
        );
        eprintln!("skip: GNU dd is not installed");
        return;
    };

    for (input, args) in [
        ("abcde", "ibs=4 obs=2 conv=sync status=noxfer"),
        ("abcd", "ibs=4 obs=2 conv=sync status=noxfer"),
        ("abcde", "bs=2 count=2 skip=1 status=noxfer"),
        (
            "abcde",
            "ibs=4 count=3 iflag=count_bytes conv=sync status=noxfer",
        ),
        ("ab", "ibs=4 conv=sync,ucase status=none"),
        ("abcde", "ibs=4 count=0 conv=sync status=none"),
        ("", "ibs=4 conv=sync status=noxfer"),
        ("ab", "ibs=4 conv=sync of=/dev/null status=noxfer"),
    ] {
        let mut host = Command::new(program)
            .args(args.split_whitespace())
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // GNU dd with count=0 exits without reading a byte, so this write
        // races the child's exit and fails with EPIPE whenever the child
        // wins. That is the case being exercised, not an error: the
        // assertions below still compare stdout, stderr and exit code.
        let mut stdin = host.stdin.take().unwrap();
        match stdin.write_all(input.as_bytes()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(error) => panic!("writing dd stdin for {args}: {error}"),
        }
        drop(stdin); // close before wait_with_output, as the temporary did
        let expected = host.wait_with_output().unwrap();
        let actual = Bash::new()
            .exec_with_options(&format!("dd {args}"), ExecOptions::new().stdin(input))
            .await
            .unwrap();
        assert_eq!(actual.stdout.as_bytes(), expected.stdout, "{args}");
        assert_eq!(actual.stderr.as_bytes(), expected.stderr, "{args}");
        assert_eq!(actual.exit_code, expected.status.code().unwrap(), "{args}");
    }
}

#[tokio::test]
async fn dd_padding_keeps_seek_and_file_errors() {
    let mut bash = Bash::new();
    bash.fs()
        .write_file(std::path::Path::new("/copy"), b"abcdef")
        .await
        .unwrap();
    let result = bash
        .exec("printf XY | dd of=/copy ibs=4 obs=1 seek=1 conv=sync,notrunc status=none")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    assert_eq!(
        bash.fs()
            .read_file(std::path::Path::new("/copy"))
            .await
            .unwrap(),
        b"aXY\0\0f"
    );

    for (script, stderr) in [
        (
            "printf XY | dd of=/copy ibs=4 conv=sync,excl status=none",
            "dd: failed to open '/copy': File exists\n",
        ),
        (
            "printf XY | dd of=/missing ibs=4 conv=sync,nocreat status=none",
            "dd: failed to open '/missing': No such file or directory\n",
        ),
    ] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, stderr);
        assert!(result.stdout.is_empty());
    }
    assert_eq!(
        bash.fs()
            .read_file(std::path::Path::new("/copy"))
            .await
            .unwrap(),
        b"aXY\0\0f"
    );
}
