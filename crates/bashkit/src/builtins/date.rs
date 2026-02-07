//! Date builtin - display or format date and time
//!
//! SECURITY: Format strings are validated before use to prevent panics.
//! Invalid format specifiers result in an error message, not a crash.
//! Additionally, runtime format errors (e.g., timezone unavailable) are
//! caught and return graceful errors.

use std::fmt::Write;

use async_trait::async_trait;
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The date builtin - display or set date and time.
///
/// Usage: date [+FORMAT] [-u]
///
/// Options:
///   +FORMAT  Output date according to FORMAT
///   -u       Display UTC time instead of local time
///
/// FORMAT specifiers:
///   %Y  Year with century (e.g., 2024)
///   %m  Month (01-12)
///   %d  Day of month (01-31)
///   %H  Hour (00-23)
///   %M  Minute (00-59)
///   %S  Second (00-59)
///   %s  Seconds since Unix epoch
///   %a  Abbreviated weekday name
///   %A  Full weekday name
///   %b  Abbreviated month name
///   %B  Full month name
///   %c  Date and time representation
///   %D  Date as %m/%d/%y
///   %F  Date as %Y-%m-%d
///   %T  Time as %H:%M:%S
///   %n  Newline
///   %t  Tab
///   %%  Literal %
pub struct Date;

/// Validate a strftime format string.
/// Returns Ok(()) if valid, or an error message describing the issue.
///
/// THREAT[TM-INT-003]: chrono::format() can panic on invalid format specifiers
/// Mitigation: Pre-validate format string and return human-readable error
fn validate_format(format: &str) -> std::result::Result<(), String> {
    // StrftimeItems parses the format string and yields Item::Error for invalid specifiers
    for item in StrftimeItems::new(format) {
        if let Item::Error = item {
            return Err(format!("invalid format string: '{}'", format));
        }
    }
    Ok(())
}

/// Parse a date string like GNU date's -d flag.
/// Supports: "now", "yesterday", "tomorrow", "N days ago", "+N days",
/// "N weeks ago", "N months ago", "N years ago", "N hours ago",
/// "@EPOCH", "YYYY-MM-DD", "YYYY-MM-DD HH:MM:SS"
fn parse_date_string(s: &str) -> std::result::Result<DateTime<Utc>, String> {
    let s = s.trim();
    let lower = s.to_lowercase();
    let now = Utc::now();

    // Epoch timestamp: @1234567890
    if let Some(epoch_str) = s.strip_prefix('@') {
        let ts: i64 = epoch_str
            .trim()
            .parse()
            .map_err(|_| format!("invalid date '{}'", s))?;
        return DateTime::from_timestamp(ts, 0).ok_or_else(|| format!("invalid date '{}'", s));
    }

    // Special words
    match lower.as_str() {
        "now" => return Ok(now),
        "yesterday" => return Ok(now - Duration::days(1)),
        "tomorrow" => return Ok(now + Duration::days(1)),
        _ => {}
    }

    // Relative: "N unit(s) ago" or "+N unit(s)" or "-N unit(s)"
    if let Some(duration) = parse_relative_date(&lower) {
        return Ok(now + duration);
    }

    // Try ISO-like formats: YYYY-MM-DD HH:MM:SS, YYYY-MM-DD
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&dt));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&dt));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = d
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("invalid date '{}'", s))?;
        return Ok(Utc.from_utc_datetime(&dt));
    }

    // Try "Mon DD, YYYY" format
    if let Ok(d) = NaiveDate::parse_from_str(s, "%b %d, %Y") {
        let dt = d
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("invalid date '{}'", s))?;
        return Ok(Utc.from_utc_datetime(&dt));
    }

    Err(format!("date: invalid date '{}'", s))
}

/// Parse relative date expressions like "30 days ago", "+2 weeks", "-1 month"
fn parse_relative_date(s: &str) -> Option<Duration> {
    // "N unit(s) ago"
    let re_ago =
        regex::Regex::new(r"^(\d+)\s+(second|minute|hour|day|week|month|year)s?\s+ago$").ok()?;
    if let Some(caps) = re_ago.captures(s) {
        let n: i64 = caps[1].parse().ok()?;
        return Some(unit_duration(&caps[2], -n));
    }

    // "+N unit(s)" or "-N unit(s)" or "N unit(s)"
    let re_rel =
        regex::Regex::new(r"^([+-]?)(\d+)\s+(second|minute|hour|day|week|month|year)s?$").ok()?;
    if let Some(caps) = re_rel.captures(s) {
        let sign = if &caps[1] == "-" { -1i64 } else { 1i64 };
        let n: i64 = caps[2].parse().ok()?;
        return Some(unit_duration(&caps[3], sign * n));
    }

    // "next unit" / "last unit"
    if let Some(unit) = s.strip_prefix("next ") {
        let unit = unit.trim().trim_end_matches('s');
        return Some(unit_duration(unit, 1));
    }
    if let Some(unit) = s.strip_prefix("last ") {
        let unit = unit.trim().trim_end_matches('s');
        return Some(unit_duration(unit, -1));
    }

    None
}

fn unit_duration(unit: &str, n: i64) -> Duration {
    match unit {
        "second" => Duration::seconds(n),
        "minute" => Duration::minutes(n),
        "hour" => Duration::hours(n),
        "day" => Duration::days(n),
        "week" => Duration::weeks(n),
        "month" => Duration::days(n * 30), // Approximate
        "year" => Duration::days(n * 365), // Approximate
        _ => Duration::zero(),
    }
}

#[async_trait]
impl Builtin for Date {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut utc = false;
        let mut format_arg: Option<String> = None;
        let mut date_str: Option<String> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-u" || arg == "--utc" {
                utc = true;
            } else if arg == "-d" || arg == "--date" {
                i += 1;
                if i < ctx.args.len() {
                    date_str = Some(ctx.args[i].clone());
                }
            } else if let Some(val) = arg.strip_prefix("--date=") {
                date_str = Some(val.to_string());
            } else if arg.starts_with('+') {
                format_arg = Some(arg.clone());
            }
            i += 1;
        }

        let default_format = "%a %b %e %H:%M:%S %Z %Y".to_string();
        let format = match &format_arg {
            Some(fmt) => &fmt[1..], // Strip leading '+'
            None => &default_format,
        };

        // SECURITY: Validate format string before use to prevent panics
        // THREAT[TM-INT-003]: Invalid format strings could cause chrono to panic
        if let Err(e) = validate_format(format) {
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: format!("date: {}\n", e),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
            });
        }

        // Format the date, handling potential errors gracefully.
        let mut output = String::new();
        let format_result = if let Some(ref ds) = date_str {
            // Parse the date string
            match parse_date_string(ds) {
                Ok(dt) => {
                    if utc {
                        write!(output, "{}", dt.format(format))
                    } else {
                        let local_dt: DateTime<Local> = dt.into();
                        write!(output, "{}", local_dt.format(format))
                    }
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("{}\n", e), 1));
                }
            }
        } else if utc {
            let now = Utc::now();
            write!(output, "{}", now.format(format))
        } else {
            let now = Local::now();
            write!(output, "{}", now.format(format))
        };

        match format_result {
            Ok(()) => Ok(ExecResult::ok(format!("{}\n", output))),
            Err(_) => Ok(ExecResult::err(
                format!("date: failed to format date with '{}'\n", format),
                1,
            )),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_date(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Date.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_date_default() {
        let result = run_date(&[]).await;
        assert_eq!(result.exit_code, 0);
        // Just check it outputs something with a newline
        assert!(result.stdout.ends_with('\n'));
        assert!(result.stdout.len() > 10);
    }

    #[tokio::test]
    async fn test_date_format_year() {
        let result = run_date(&["+%Y"]).await;
        assert_eq!(result.exit_code, 0);
        // Should be a 4-digit year
        let year = result.stdout.trim();
        assert_eq!(year.len(), 4);
        assert!(year.chars().all(|c| c.is_ascii_digit()));
    }

    #[tokio::test]
    async fn test_date_format_iso() {
        let result = run_date(&["+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        // Should be like 2024-01-15
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
        assert!(date.chars().nth(4) == Some('-'));
        assert!(date.chars().nth(7) == Some('-'));
    }

    #[tokio::test]
    async fn test_date_epoch() {
        let result = run_date(&["+%s"]).await;
        assert_eq!(result.exit_code, 0);
        // Should be a valid unix timestamp (10 digits or more)
        let epoch = result.stdout.trim();
        assert!(epoch.len() >= 10);
        assert!(epoch.parse::<i64>().is_ok());
    }

    #[tokio::test]
    async fn test_date_utc() {
        let result = run_date(&["-u", "+%Z"]).await;
        assert_eq!(result.exit_code, 0);
        // Should show UTC timezone
        let tz = result.stdout.trim();
        assert!(tz.contains("UTC") || tz == "+0000" || tz == "+00:00");
    }

    #[tokio::test]
    async fn test_date_time_format() {
        let result = run_date(&["+%H:%M:%S"]).await;
        assert_eq!(result.exit_code, 0);
        // Should be like 12:34:56
        let time = result.stdout.trim();
        assert_eq!(time.len(), 8);
        let parts: Vec<&str> = time.split(':').collect();
        assert_eq!(parts.len(), 3);
    }

    // Tests from main: timezone handling
    #[tokio::test]
    async fn test_date_timezone_utc() {
        // %Z with UTC should always work and produce "UTC"
        let result = run_date(&["-u", "+%Z"]).await;
        assert_eq!(result.exit_code, 0);
        let tz = result.stdout.trim();
        assert!(tz.contains("UTC") || tz == "+0000" || tz == "+00:00");
    }

    #[tokio::test]
    async fn test_date_default_format_includes_timezone() {
        // The default format includes %Z - this tests that it doesn't panic
        let result = run_date(&[]).await;
        assert_eq!(result.exit_code, 0);
        // Default format: "%a %b %e %H:%M:%S %Z %Y"
        // Should contain a year
        let output = result.stdout.trim();
        assert!(
            output.len() > 15,
            "Default format should produce substantial output"
        );
    }

    #[tokio::test]
    async fn test_date_timezone_local() {
        // %Z with local time - this is the case that can fail in some environments
        // With our fix, it should either succeed or return a graceful error
        let result = run_date(&["+%Z"]).await;
        // Either succeeds with exit_code 0, or fails gracefully with exit_code 1
        if result.exit_code == 0 {
            // Successful: output should be non-empty
            assert!(!result.stdout.trim().is_empty());
        } else {
            // Failed gracefully: should have error message
            assert!(result.stderr.contains("date:"));
            assert!(result.stderr.contains("failed to format"));
        }
    }

    #[tokio::test]
    async fn test_date_combined_format_with_timezone() {
        // Test combination of formats including %Z
        let result = run_date(&["-u", "+%Y-%m-%d %H:%M:%S %Z"]).await;
        assert_eq!(result.exit_code, 0);
        let output = result.stdout.trim();
        // Should have date, time, and timezone
        assert!(output.contains('-')); // Date separator
        assert!(output.contains(':')); // Time separator
    }

    #[tokio::test]
    async fn test_date_empty_format() {
        // Empty format string (just "+")
        let result = run_date(&["+"]).await;
        assert_eq!(result.exit_code, 0);
        // Should produce just a newline
        assert_eq!(result.stdout, "\n");
    }

    #[tokio::test]
    async fn test_date_literal_text_in_format() {
        // Format with literal text
        let result = run_date(&["+Today is %A"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.starts_with("Today is "));
    }

    // Tests for invalid format validation (TM-INT-003)
    #[tokio::test]
    async fn test_date_invalid_format_specifier() {
        // Invalid format specifier should return error, not panic
        let result = run_date(&["+%Q"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid format string"));
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_date_incomplete_format_specifier() {
        // Incomplete specifier at end should return error, not panic
        let result = run_date(&["+%Y-%m-%"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid format string"));
    }

    #[tokio::test]
    async fn test_date_mixed_valid_invalid_format() {
        // Mix of valid and invalid should still error
        let result = run_date(&["+%Y-%Q-%d"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid format string"));
    }

    // === Tests for -d / --date flag ===

    #[tokio::test]
    async fn test_date_d_now() {
        let result = run_date(&["-d", "now", "+%Y"]).await;
        assert_eq!(result.exit_code, 0);
        let year = result.stdout.trim();
        assert_eq!(year.len(), 4);
    }

    #[tokio::test]
    async fn test_date_d_yesterday() {
        let result = run_date(&["-d", "yesterday", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
    }

    #[tokio::test]
    async fn test_date_d_tomorrow() {
        let result = run_date(&["-d", "tomorrow", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
    }

    #[tokio::test]
    async fn test_date_d_days_ago() {
        let result = run_date(&["-d", "30 days ago", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
    }

    #[tokio::test]
    async fn test_date_d_epoch() {
        let result = run_date(&["-d", "@0", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "1970-01-01");
    }

    #[tokio::test]
    async fn test_date_d_iso_date() {
        let result = run_date(&["-d", "2024-01-15", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "2024-01-15");
    }

    #[tokio::test]
    async fn test_date_d_iso_datetime() {
        let result = run_date(&["-d", "2024-06-15 14:30:00", "+%H:%M"]).await;
        assert_eq!(result.exit_code, 0);
        // In UTC mode this is exact; in local mode it depends on timezone
        assert!(result.stdout.trim().contains(':'));
    }

    #[tokio::test]
    async fn test_date_d_invalid() {
        let result = run_date(&["-d", "not a date"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid date"));
    }

    #[tokio::test]
    async fn test_date_d_relative_weeks() {
        let result = run_date(&["-d", "2 weeks ago", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
    }

    #[tokio::test]
    async fn test_date_d_plus_days() {
        let result = run_date(&["-d", "+7 days", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
    }

    #[tokio::test]
    async fn test_date_long_date_flag() {
        let result = run_date(&["--date=yesterday", "+%Y-%m-%d"]).await;
        assert_eq!(result.exit_code, 0);
        let date = result.stdout.trim();
        assert_eq!(date.len(), 10);
    }
}
