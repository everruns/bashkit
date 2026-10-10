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

#[tokio::test]
async fn shared_iso_date_parsing_preserves_instants_across_builtins() {
    for (input, expected) in [
        ("2024-01-15 10:40", "2024-01-15 16:40:00.000000000"),
        ("2024-01-15 10:40 UTC", "2024-01-15 10:40:00.000000000"),
        ("2024-01-15 10:40 gmt", "2024-01-15 10:40:00.000000000"),
        ("2024-01-15T10:40Z", "2024-01-15 10:40:00.000000000"),
        (
            "2024-01-15 10:40:00.5 +0200",
            "2024-01-15 08:40:00.500000000",
        ),
        ("2024-01-15 10:40 -05", "2024-01-15 15:40:00.000000000"),
    ] {
        let script = format!(
            "date -u -d '{input}' '+%F %T.%N'; \
             touch -d '{input}' /tmp/stamp && date -u -r /tmp/stamp '+%F %T.%N'; \
             find /tmp/stamp -newermt '{input}'; \
             find /tmp/stamp -newermt '{input} + 1 second'; \
             find /tmp/stamp -newermt '{input} - 1 second'"
        );
        let result = fixed_date(Some("America/Chicago"), &script).await;
        assert_eq!(result.exit_code, 0, "{input}: {}", result.stderr);
        assert!(result.stderr.is_empty(), "{input}: {}", result.stderr);
        assert_eq!(
            result.stdout,
            format!("{expected}\n{expected}\n/tmp/stamp\n"),
            "{input}"
        );
    }
}

#[tokio::test]
async fn shared_iso_date_parsing_rejects_invalid_inputs_before_file_effects() {
    for input in [
        "2024-03-10 02:30", // DST gap in the sandbox timezone.
        "2024-02-30 10:40",
        "2024-07-15 24:00",
        "2024-07-15 10:40 +2500",
        "2024-07-15 10:40 +0260",
        "2024-07-15 10:40 UTC trailing",
        "2024-07-15 10:40 /etc/localtime",
    ] {
        let mut bash = Bash::builder().env("TZ", "America/Chicago").build();
        for script in [
            format!("date -d '{input}' +%s"),
            format!("touch -d '{input}' /tmp/rejected"),
            format!("find /tmp -newermt '{input}'"),
        ] {
            let result = bash.exec(&script).await.unwrap();
            assert_eq!(result.exit_code, 1, "{script}: {}", result.stderr);
            assert!(result.stdout.is_empty(), "{script}: {}", result.stdout);
            assert!(!result.stderr.is_empty(), "{script}");
        }
        let result = bash.exec("test -e /tmp/rejected").await.unwrap();
        assert_eq!(result.exit_code, 1, "invalid touch created a file: {input}");
    }
}

#[tokio::test]
async fn shared_weekday_parsing_uses_sandbox_calendar_and_clock() {
    // Monday in UTC and Sydney, still Sunday in Chicago.
    const EPOCH: i64 = 1_705_277_400; // 2024-01-15 00:10 UTC
    for (tz, input, expected) in [
        ("UTC", "monday", "2024-01-15 00:00:00 +0000"),
        ("UTC", "this Mon", "2024-01-15 00:00:00 +0000"),
        ("UTC", "next monday", "2024-01-22 00:00:00 +0000"),
        ("UTC", "last monday", "2024-01-08 00:00:00 +0000"),
        ("America/Chicago", "sunday", "2024-01-14 00:00:00 -0600"),
        ("America/Chicago", "next sun", "2024-01-21 00:00:00 -0600"),
        ("America/Chicago", "last  SUN", "2024-01-07 00:00:00 -0600"),
        ("Australia/Sydney", "MONDAY", "2024-01-15 00:00:00 +1100"),
    ] {
        let mut bash = Bash::builder().fixed_epoch(EPOCH).env("TZ", tz).build();
        let result = bash.exec(&format!(
            "date -d '{input}' '+%F %T %z'; touch -d '{input}' /tmp/stamp; date -r /tmp/stamp '+%F %T %z'; find /tmp/stamp -newermt '{input} - 1 second'; find /tmp/stamp -newermt '{input}'"
        )).await.unwrap();
        assert_eq!(result.exit_code, 0, "TZ={tz} {input}: {}", result.stderr);
        assert_eq!(
            result.stdout,
            format!("{expected}\n{expected}\n/tmp/stamp\n")
        );
        assert!(result.stderr.is_empty());
    }
    let mut bash = Bash::builder()
        .fixed_epoch(EPOCH)
        .env("TZ", "America/Chicago")
        .build();
    let result = bash
        .exec("date -d today +%s; touch -d today /tmp/stamp; date -r /tmp/stamp +%s")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, format!("{EPOCH}\n{EPOCH}\n"));
}

#[tokio::test]
async fn shared_weekday_parsing_rejects_unknown_words_before_file_effects() {
    for input in [
        "next blursday",
        "last blah",
        "next mins",
        "this nonsense",
        "next monday trailing",
        "next /etc/localtime",
    ] {
        let mut bash = Bash::builder().fixed_epoch(WINTER_EPOCH).build();
        for script in [
            format!("date -d '{input}' +%s"),
            format!("touch -d '{input}' /tmp/rejected"),
            format!("find /tmp -newermt '{input}'"),
        ] {
            let result = bash.exec(&script).await.unwrap();
            assert_eq!(result.exit_code, 1, "{script}: {}", result.stderr);
            assert!(result.stdout.is_empty(), "{script}: {}", result.stdout);
            assert!(!result.stderr.is_empty());
        }
        assert_eq!(
            bash.exec("test -e /tmp/rejected").await.unwrap().exit_code,
            1
        );
    }
}

#[tokio::test]
async fn weekday_midnight_observes_dst_rules() {
    let mut bash = Bash::builder()
        .fixed_epoch(1_710_093_600)
        .env("TZ", "America/Chicago")
        .build(); // 2024-03-10 18:00 UTC
    let result = bash
        .exec("date -d sunday '+%F %T %z'; date -d 'next sunday' '+%F %T %z'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout,
        "2024-03-10 00:00:00 -0600\n2024-03-17 00:00:00 -0500\n"
    );
    // Sao Paulo skipped midnight on this date: reject instead of inventing an instant.
    let mut bash = Bash::builder()
        .fixed_epoch(1_541_332_800)
        .env("TZ", "America/Sao_Paulo")
        .build(); // 2018-11-04 12:00 UTC
    let result = bash.exec("date -d sunday +%s").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
}
