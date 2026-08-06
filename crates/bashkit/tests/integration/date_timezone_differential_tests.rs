//! Portable differential checks against GNU `date` when available.

use std::path::PathBuf;
use std::process::Command;

use bashkit::Bash;

fn gnu_date() -> Option<PathBuf> {
    for candidate in ["gdate", "date"] {
        let Ok(output) = Command::new(candidate).arg("--version").output() else {
            continue;
        };
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains("GNU coreutils")
        {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

async fn assert_gnu_match(tz: &str, args: &[&str]) {
    let Some(program) = gnu_date() else {
        eprintln!("skip: GNU date is not installed");
        return;
    };

    let host = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("TZ", tz)
        .output()
        .expect("run GNU date");

    let command = std::iter::once("date".to_string())
        .chain(args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut bash = Bash::builder().env("TZ", tz).build();
    let actual = bash.exec(&command).await.unwrap();

    assert_eq!(
        actual.exit_code,
        host.status.code().unwrap_or(-1),
        "{command}"
    );
    assert_eq!(actual.stdout.as_bytes(), host.stdout, "{command}");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[tokio::test]
async fn matches_gnu_date_for_timezone_sensitive_inputs() {
    for (tz, args) in [
        (
            "America/Chicago",
            &["-d", "@1710057600", "+%F %T %Z %z"] as &[&str],
        ),
        (
            "America/Chicago",
            &["-d", "2024-01-15 10:40:00", "+%s %F %T %Z %z"],
        ),
        (
            "America/Chicago",
            &["-d", "2024-01-15T12:00:00+02:00", "+%s %T %Z %z"],
        ),
        (
            "America/Chicago",
            &["-u", "-d", "2024-01-15T10:40:00-06:00", "+%s %T %Z %z"],
        ),
        ("Etc/GMT+6", &["-d", "@0", "+%F %T %Z %z"]),
        ("UTC", &["-d", "@0", "+%C %G %g %u %V %3N %6N %N"]),
    ] {
        assert_gnu_match(tz, args).await;
    }
}
