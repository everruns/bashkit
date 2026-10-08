//! od dimensions and output must be bounded before allocation.

use bashkit::{Bash, Error, ExecutionLimits, LimitExceeded};

#[tokio::test]
async fn od_rejects_width_above_ceiling() {
    let mut bash = Bash::new();
    let result = bash.exec("printf x | od -w65538").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stderr, "od: width exceeds maximum of 65536 bytes\n");
    assert!(result.stdout.is_empty());
}

#[tokio::test]
async fn od_trailer_output_respects_live_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(4096))
        .build();
    let result = bash.exec("printf x | od -An -tx1z -w65536").await;
    assert!(matches!(
        result,
        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
    ));
    let recovery = bash.exec("printf x | od -An -tx1").await.unwrap();
    assert_eq!(recovery.stdout, " 78\n");
}

#[tokio::test]
async fn od_enormous_widths_fail_safely_and_leave_host_usable() {
    let mut bash = Bash::new();
    for width in [
        "1152921504606846976",
        "0x1000000000000000",
        "01000000000000000000000",
        "1125899906842624k",
        "1099511627776m",
        "18446744073709551615",
    ] {
        for input in ["printf x |", "printf '' |"] {
            for flags in ["", "-tx1z", "-tu8", "-tc"] {
                let script = format!("{input} od {flags} --width={width}");
                let result = bash.exec(&script).await.unwrap();
                assert_eq!(result.exit_code, 1, "{script}");
                assert!(result.stdout.is_empty(), "{script}");
                assert!(result.stderr.starts_with("od:"), "{script}");
                assert!(result.stderr.len() < 1024);
            }
        }
    }
    let result = bash.exec("printf x | od -An -tx1").await.unwrap();
    assert_eq!(result.stdout, " 78\n");
}

#[tokio::test]
async fn od_width_boundary_needs_only_actual_field_storage() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(4096))
        .build();
    for width in ["65536", "0x10000", "0200000", "64k"] {
        let result = bash
            .exec(&format!("printf x | od -An -tx8 --width={width}"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, " 0000000000000078\n");
    }
}

#[tokio::test]
async fn od_work_budget_bounds_rendering() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_work_units(50))
        .build();
    bash.fs()
        .write_file(std::path::Path::new("/input"), &[b'x'; 64])
        .await
        .unwrap();
    assert!(matches!(
        bash.exec("od -An -tx1 /input").await,
        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
    ));
}

#[tokio::test]
async fn od_normal_layout_matches_gnu() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let Some(program) = ["god", "od"].into_iter().find(|program| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|out| {
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout).contains("GNU coreutils")
            })
    }) else {
        eprintln!("skip: GNU od is not installed");
        return;
    };
    // Include partial numeric fields, mixed-size alignment, endian, trailers,
    // duplicate suppression, and GNU's zero/misaligned-width fallback.
    let mut bash = Bash::new();
    for args in [
        "-An -tx1 -w4",
        "-tx2z -w16",
        "-An -tx8 --endian=big",
        "-An -tc -tx8 -w32",
        "-An -tx2 -w0",
        "-An -tx2 -w3",
        "-An -v -tx1 -w4",
        "-An -tx8 -w65536",
        "-An -ta -td1 -w65536",
    ] {
        for input in ["abc", "aaaaaaaaaaaa", ""] {
            let mut host = Command::new(program)
                .args(args.split_whitespace())
                .env("LC_ALL", "C")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            host.stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let expected = host.wait_with_output().unwrap();
            let script = format!("printf '{input}' | od {args}");
            let actual = bash.exec(&script).await.unwrap();
            assert_eq!(
                actual.exit_code,
                expected.status.code().unwrap(),
                "{script}"
            );
            assert_eq!(actual.stdout.as_bytes(), expected.stdout, "{script}");
        }
    }
}

#[tokio::test]
async fn od_pipeline_output_is_not_bounded_by_capture_cap() {
    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_stdout_bytes(32)
                .max_live_intermediate_bytes(32_768),
        )
        .build();
    let result = bash
        .exec("printf x | od -An -tx1z -w2048 | wc -c")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "6150");
    assert!(result.stderr.is_empty());
}
