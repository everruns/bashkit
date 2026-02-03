//! Date builtin - display or format date and time

use std::fmt::Write;

use async_trait::async_trait;
use chrono::{Local, Utc};

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

#[async_trait]
impl Builtin for Date {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let utc = ctx.args.iter().any(|a| a == "-u" || a == "--utc");
        let format_arg = ctx.args.iter().find(|a| a.starts_with('+'));

        let format = match format_arg {
            Some(fmt) => &fmt[1..],            // Strip leading '+'
            None => "%a %b %e %H:%M:%S %Z %Y", // Default format like: "Mon Jan  1 12:00:00 UTC 2024"
        };

        // Format the date, handling potential errors gracefully.
        // chrono's Display::fmt can return an error (e.g., when %Z timezone name
        // cannot be determined), which would cause to_string() to panic.
        // We use write! to a String instead, which returns a Result.
        let mut output = String::new();
        let format_result = if utc {
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
}
