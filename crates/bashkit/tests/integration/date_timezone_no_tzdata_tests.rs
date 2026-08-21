//! `date` timezone behavior when the `tzdata` feature is off.
//!
//! Without chrono-tz there is no IANA database, so a named zone cannot be
//! resolved. `date` says so and exits 1 rather than quietly formatting in UTC:
//! a wrong timestamp that looks right is worse than a refusal.
//!
//! The fail-closed direction of THREAT[TM-INF-018] is unchanged -- nothing here
//! consults or reveals host timezone state, and the diagnostic never echoes the
//! `TZ` value, which may itself be a host path.

use bashkit::Bash;

const WINTER_EPOCH: i64 = 1_705_315_200; // 2024-01-15 10:40:00 UTC
const UNAVAILABLE: &str = "named timezones are unavailable";

async fn fixed_date(tz: Option<&str>, script: &str) -> bashkit::ExecResult {
    let mut builder = Bash::builder().fixed_epoch(WINTER_EPOCH);
    if let Some(tz) = tz {
        builder = builder.env("TZ", tz);
    }
    builder.build().exec(script).await.unwrap()
}

#[tokio::test]
async fn named_zone_errors_instead_of_silently_using_utc() {
    let result = fixed_date(Some("America/Chicago"), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains(UNAVAILABLE),
        "stderr was {:?}",
        result.stderr
    );
    // Critically: no timestamp on stdout. A caller that ignores the exit code
    // must not be handed a plausible-looking wrong time.
    assert!(result.stdout.is_empty(), "stdout was {:?}", result.stdout);
}

#[tokio::test]
async fn named_zone_error_does_not_echo_the_tz_value() {
    // Mirrors invalid_timezone_fails_closed_without_echoing_host_state: a TZ can
    // be path-shaped, so it must never be reflected back.
    for tz in [
        "America/Chicago",
        "Not/A_Real_Zone",
        "CST6CDT,M3.2.0,M11.1.0",
    ] {
        let result = fixed_date(Some(tz), "date '+%Y-%m-%d'").await;
        assert!(!result.stdout.contains(tz), "TZ value leaked to stdout");
        assert!(!result.stderr.contains(tz), "TZ value leaked to stderr");
    }
}

#[tokio::test]
async fn unrecognized_zone_also_errors() {
    // With no database a typo and a real zone are indistinguishable, so both
    // take the loud path. (With `tzdata` on, a typo resolves to UTC silently --
    // matching GNU date's leniency, which only makes sense when a database
    // exists to be lenient about.)
    let result = fixed_date(Some("Not/A_Real_Zone"), "date '+%Y-%m-%d'").await;
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains(UNAVAILABLE));
}

#[tokio::test]
async fn utc_flag_does_not_excuse_an_unresolvable_tz() {
    // `-u` overrides only the *display* zone; TZ still governs naive `-d`
    // parsing, so an unresolvable TZ is still an error under `-u`.
    let result = fixed_date(
        Some("America/Chicago"),
        "date -u -d '2024-01-15 10:40:00' +%s",
    )
    .await;
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains(UNAVAILABLE));
}

#[tokio::test]
async fn unset_empty_and_utc_still_work() {
    for tz in [None, Some(""), Some("UTC")] {
        let result = fixed_date(tz, "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
        assert_eq!(result.exit_code, 0, "TZ={tz:?}: {}", result.stderr);
        assert_eq!(result.stdout.trim(), "2024-01-15 10:40:00 UTC +0000");
    }
}

#[tokio::test]
async fn path_style_timezone_still_fails_closed_silently() {
    // These are rejected requests for a host zoneinfo file, not requests for a
    // named zone this build cannot serve. They resolve to UTC in every build,
    // so the behavior must not diverge with the feature.
    for tz in [":/etc/localtime", "../../etc/localtime"] {
        let result = fixed_date(Some(tz), "date '+%Y-%m-%d %H:%M:%S %Z %z'").await;
        assert_eq!(result.exit_code, 0, "TZ={tz}: {}", result.stderr);
        assert_eq!(
            result.stdout.trim(),
            "2024-01-15 10:40:00 UTC +0000",
            "TZ={tz}"
        );
        assert!(!result.stderr.contains(tz), "TZ value leaked to stderr");
    }
}

#[tokio::test]
async fn explicit_input_offset_still_needs_a_resolvable_tz() {
    // The offset in the input is authoritative for the instant, but TZ is still
    // consulted, so the command errors rather than half-honoring the request.
    let result = fixed_date(
        Some("America/Chicago"),
        "date -d '2024-01-15T12:00:00+02:00' '+%H:%M %Z %z %s'",
    )
    .await;
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains(UNAVAILABLE));

    // Without a TZ set, the same input resolves normally.
    let result = fixed_date(
        None,
        "date -d '2024-01-15T12:00:00+02:00' '+%H:%M %Z %z %s'",
    )
    .await;
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "10:00 UTC +0000 1705312800");
}

#[tokio::test]
async fn error_is_a_clean_diagnostic_not_a_panic_or_debug_dump() {
    // TM-INF-022: stderr from builtins must not leak internal Debug shapes.
    let result = fixed_date(Some("Europe/Kyiv"), "date").await;
    assert_eq!(result.exit_code, 1);
    let stderr = String::from_utf8_lossy(result.stderr.as_ref()).to_string();
    assert!(stderr.starts_with("date: "), "stderr was {stderr:?}");
    assert!(stderr.ends_with('\n'));
    assert!(stderr.len() < 200, "diagnostic too long: {stderr:?}");
    assert!(!stderr.contains('{'), "looks like a Debug dump: {stderr:?}");
}
