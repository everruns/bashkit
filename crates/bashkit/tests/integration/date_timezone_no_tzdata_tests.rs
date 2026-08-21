//! `date` timezone behavior when the `tzdata` feature is off.
//!
//! Without chrono-tz there is no IANA database to resolve against, so the
//! closed timezone set of THREAT[TM-INF-018] narrows to UTC alone. A named
//! zone then resolves the way an unrecognised one always has -- to UTC --
//! rather than falling back to host-local state. These tests pin that, so the
//! slim build cannot start honouring host timezones unnoticed.

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
async fn named_zone_resolves_to_utc() {
    let result = fixed_date(Some("America/Chicago"), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "2024-01-15 10:40:00 UTC +0000");
}

#[tokio::test]
async fn named_zone_does_not_shift_naive_parsing() {
    // With `tzdata` this parses as America/Chicago and yields 1705336800.
    let result = fixed_date(Some("America/Chicago"), "date -d '2024-01-15 10:40:00' +%s").await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "1705315200");
}

#[tokio::test]
async fn named_zone_matches_unset_and_utc_and_invalid() {
    let named = fixed_date(Some("Europe/Kyiv"), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
    let unset = fixed_date(None, "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
    let utc = fixed_date(Some("UTC"), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
    let invalid = fixed_date(Some("Not/AZone"), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;

    assert_eq!(named.stdout, unset.stdout);
    assert_eq!(named.stdout, utc.stdout);
    assert_eq!(named.stdout, invalid.stdout);
}

#[tokio::test]
async fn utc_flag_still_overrides_display() {
    let result = fixed_date(
        Some("America/Chicago"),
        "date -u -d '2024-01-15 10:40:00' '+%s %H:%M %Z %z'",
    )
    .await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "1705315200 10:40 UTC +0000");
}

#[tokio::test]
async fn explicit_input_offset_remains_authoritative() {
    // An offset carried in the input string is parsed by the format, not the
    // zone database, so it must survive the feature being off.
    // Same input as the `tzdata` counterpart, which reads it back as
    // "04:00 CST -0600 1705312800". The instant is identical either way; only
    // the display zone changes.
    let result = fixed_date(
        Some("America/Chicago"),
        "date -d '2024-01-15T12:00:00+02:00' '+%H:%M %Z %z %s'",
    )
    .await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "10:00 UTC +0000 1705312800");
}

#[tokio::test]
async fn path_style_timezone_still_fails_closed() {
    for tz in [":/etc/localtime", "../../etc/localtime"] {
        let result = fixed_date(Some(tz), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
        assert_eq!(result.exit_code, 0, "TZ={tz}: {}", result.stderr);
        assert_eq!(
            result.stdout.trim(),
            "2024-01-15 10:40:00 UTC +0000",
            "TZ={tz}"
        );
    }
}
