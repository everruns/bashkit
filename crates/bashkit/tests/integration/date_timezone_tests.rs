//! `date` timezone compatibility and information-disclosure tests.

use bashkit::Bash;

const WINTER_EPOCH: i64 = 1_705_315_200; // 2024-01-15 10:40:00 UTC

async fn fixed_date(tz: Option<&str>, script: &str) -> bashkit::ExecResult {
    let mut builder = Bash::builder().fixed_epoch(WINTER_EPOCH);
    if let Some(tz) = tz {
        builder = builder.env("TZ", tz);
    }
    builder.build().exec(script).await.unwrap()
}

#[tokio::test]
async fn unset_timezone_is_utc() {
    let result = fixed_date(None, "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "2024-01-15 10:40:00 UTC +0000");
}

#[tokio::test]
async fn iana_timezone_controls_display_and_naive_parsing() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date '+%Y-%m-%d %H:%M:%S %Z %z'; date -d '2024-01-15 10:40:00' +%s",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "2024-01-15 04:40:00 CST -0600\n1705336800\n");
}

#[tokio::test]
async fn utc_flag_only_overrides_display_timezone() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date -u -d '2024-01-15 10:40:00' '+%s %H:%M %Z %z'",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "1705336800 16:40 UTC +0000");
}

#[tokio::test]
async fn explicit_input_offset_remains_authoritative() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date -d '2024-01-15T12:00:00+02:00' '+%H:%M %Z %z %s'",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "04:00 CST -0600 1705312800");
}

#[tokio::test]
async fn invalid_timezone_fails_closed_without_echoing_host_state() {
    for tz in [
        "../../etc/localtime",
        ":/etc/localtime",
        "Not/A_Real_Zone",
        "CST6CDT,M3.2.0,M11.1.0",
    ] {
        let result = fixed_date(Some(tz), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
        assert_eq!(result.exit_code, 0, "TZ={tz}: {}", result.stderr);
        assert_eq!(
            result.stdout.trim(),
            "2024-01-15 10:40:00 UTC +0000",
            "TZ={tz}"
        );
        assert!(!result.stdout.contains(tz), "TZ value leaked to stdout");
        assert!(!result.stderr.contains(tz), "TZ value leaked to stderr");
    }
}

#[tokio::test]
async fn empty_and_iana_fixed_offset_zones_are_deterministic() {
    let empty = fixed_date(Some(""), "date '+%F %T %Z %z'").await;
    assert_eq!(empty.stdout.trim(), "2024-01-15 10:40:00 UTC +0000");

    let fixed = fixed_date(Some("Etc/GMT+6"), "date '+%F %T %Z %z'").await;
    assert_eq!(fixed.stdout.trim(), "2024-01-15 04:40:00 -06 -0600");
}

#[tokio::test]
async fn dst_transition_uses_zone_rules_and_rejects_gap() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date -d @1710057599 '+%H:%M:%S %Z %z'; date -d @1710057600 '+%H:%M:%S %Z %z'",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "01:59:59 CST -0600\n03:00:00 CDT -0500\n");

    let gap = fixed_date(Some("America/Chicago"), "date -d '2024-03-10 02:30:00' +%s").await;
    assert_eq!(gap.exit_code, 1);
    assert!(gap.stdout.is_empty());
    assert_eq!(gap.stderr, "date: invalid date '2024-03-10 02:30:00'\n");
}

#[tokio::test]
async fn repeated_dst_wall_time_chooses_earlier_instant() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date -d '2024-11-03 01:30:00' '+%s %Z %z'",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "1730615400 CDT -0500");
}

#[tokio::test]
async fn z_suffix_remains_authoritative() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date -d '2024-01-15T12:00:00Z' '+%s %H:%M %Z %z'",
    )
    .await;
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "1705320000 06:00 CST -0600");
}

#[tokio::test]
async fn command_scoped_timezone_assignment_is_honored() {
    let mut bash = Bash::builder().fixed_epoch(WINTER_EPOCH).build();
    let result = bash
        .exec("TZ=America/Chicago date '+%H:%M %Z %z'; date '+%H:%M %Z %z'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "04:40 CST -0600\n10:40 UTC +0000\n");
}

#[tokio::test]
async fn useful_gnu_strftime_specifiers_are_supported() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date '+%C %D %F %G %g %h %I %j %k %l %p %P %r %R %T %u %V %w %x %X %z %:z %::z %:::z %N'",
    )
    .await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        result.stdout.trim(),
        "20 01/15/24 2024-01-15 2024 24 Jan 04 015  4  4 AM am 04:40:00 AM 04:40 04:40:00 1 03 1 01/15/24 04:40:00 -0600 -06:00 -06:00:00 -06 000000000"
    );
}

#[tokio::test]
async fn gnu_nanosecond_precision_uses_validated_formatter() {
    let result = fixed_date(Some("UTC"), "date '+%3N %6N %9N'").await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "000 000000 000000000");
}
