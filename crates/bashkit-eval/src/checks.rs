// Deterministic expectation checks, ported from the original `scorer.rs`.
//
// The semantics are byte-for-byte the same as the pre-mira harness; the only
// change is the substrate: checks read a post-run `Snapshot` (tool outputs +
// directory set) plus the captured VFS `files` map, instead of a live
// `AgentTrace` + `&dyn FileSystem`. This keeps scoring synchronous and lets it
// run inside a mira `Scorer`, which only sees `&Sample` + `&Transcript`.
//
// Check reference (see knowledge/operations/eval.md):
//   exit_code:N           stdout_contains:text    stdout_regex:pattern
//   stderr_empty          file_exists:/path       dir_exists:/path
//   file_contains:/path:text   file_line_regex:/path:pattern   llm_judge:prompt
//   file_not_contains:/path:text  (file must exist and not contain text)
//   symlink:/path:target  (/path is a symlink whose target resolves to the
//                          same absolute path as `target`; relative or absolute
//                          spellings both pass)

use std::collections::BTreeMap;

use crate::snapshot::Snapshot;

/// Outcome of a single check.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub check: String,
    pub passed: bool,
    pub detail: String,
    pub weight: f64,
}

/// Aggregate of all checks for one task.
#[derive(Debug, Clone)]
pub struct CheckSummary {
    pub results: Vec<CheckResult>,
    /// Weighted sum of passed checks.
    pub score: f64,
    /// Sum of all weights.
    pub max_score: f64,
}

impl CheckSummary {
    /// True iff every individual check passed (weight-independent), matching the
    /// original `TaskScore::all_passed`.
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }

    /// Weighted pass rate in `[0, 1]`, matching the original `TaskScore::rate`.
    pub fn rate(&self) -> f64 {
        if self.max_score == 0.0 {
            1.0
        } else {
            self.score / self.max_score
        }
    }
}

/// Evaluate a list of `(check, weight)` expectations against a run snapshot.
pub fn evaluate(
    expectations: &[(String, f64)],
    snap: &Snapshot,
    files: &BTreeMap<String, String>,
) -> CheckSummary {
    let results: Vec<CheckResult> = expectations
        .iter()
        .map(|(check, weight)| evaluate_check(check, *weight, snap, files))
        .collect();
    let max_score: f64 = results.iter().map(|r| r.weight).sum();
    let score: f64 = results.iter().filter(|r| r.passed).map(|r| r.weight).sum();
    CheckSummary {
        results,
        score,
        max_score,
    }
}

pub fn evaluate_check(
    check: &str,
    weight: f64,
    snap: &Snapshot,
    files: &BTreeMap<String, String>,
) -> CheckResult {
    let (check_type, check_value) = check.split_once(':').unwrap_or((check, ""));

    match check_type {
        "exit_code" => check_exit_code(check, weight, check_value, snap),
        "stdout_contains" => check_stdout_contains(check, weight, check_value, snap),
        "stdout_regex" => check_stdout_regex(check, weight, check_value, snap),
        "stderr_empty" => check_stderr_empty(check, weight, snap),
        "file_exists" => check_file_exists(check, weight, check_value, snap, files),
        "dir_exists" => check_dir_exists(check, weight, check_value, snap),
        "file_contains" => check_file_contains(check, weight, check_value, files),
        "file_not_contains" => check_file_not_contains(check, weight, check_value, files),
        "file_line_regex" => check_file_line_regex(check, weight, check_value, files),
        "symlink" => check_symlink(check, weight, check_value, snap),
        "llm_judge" => CheckResult {
            check: check.to_string(),
            passed: true,
            detail: "llm_judge not implemented (stub, weight=0)".to_string(),
            weight: 0.0,
        },
        _ => CheckResult {
            check: check.to_string(),
            passed: false,
            detail: format!("unknown check type: {}", check_type),
            weight,
        },
    }
}

fn check_exit_code(check: &str, weight: f64, value: &str, snap: &Snapshot) -> CheckResult {
    let expected: i32 = value.parse().unwrap_or(0);
    let actual = snap.last_exit_code.unwrap_or(-1);
    CheckResult {
        check: check.to_string(),
        passed: actual == expected,
        detail: format!("expected {}, got {}", expected, actual),
        weight,
    }
}

fn check_stdout_contains(check: &str, weight: f64, value: &str, snap: &Snapshot) -> CheckResult {
    let found = snap.tool_outputs.iter().any(|t| t.stdout.contains(value));
    CheckResult {
        check: check.to_string(),
        passed: found,
        detail: if found {
            "found".to_string()
        } else {
            format!("'{}' not found in any tool output", value)
        },
        weight,
    }
}

fn check_stdout_regex(check: &str, weight: f64, value: &str, snap: &Snapshot) -> CheckResult {
    match regex::Regex::new(value) {
        Ok(re) => {
            let found = snap.tool_outputs.iter().any(|t| re.is_match(&t.stdout));
            CheckResult {
                check: check.to_string(),
                passed: found,
                detail: if found {
                    "matched".to_string()
                } else {
                    format!("pattern '{}' not matched", value)
                },
                weight,
            }
        }
        Err(e) => CheckResult {
            check: check.to_string(),
            passed: false,
            detail: format!("invalid regex: {}", e),
            weight,
        },
    }
}

fn check_stderr_empty(check: &str, weight: f64, snap: &Snapshot) -> CheckResult {
    let all_empty = snap.tool_outputs.iter().all(|t| t.stderr.is_empty());
    CheckResult {
        check: check.to_string(),
        passed: all_empty,
        detail: if all_empty {
            "all stderr empty".to_string()
        } else {
            let first_stderr = snap
                .tool_outputs
                .iter()
                .find(|t| !t.stderr.is_empty())
                .map(|t| t.stderr.clone())
                .unwrap_or_default();
            format!(
                "stderr: {}",
                first_stderr.chars().take(100).collect::<String>()
            )
        },
        weight,
    }
}

fn check_file_exists(
    check: &str,
    weight: f64,
    value: &str,
    snap: &Snapshot,
    files: &BTreeMap<String, String>,
) -> CheckResult {
    // `stat`-style existence: a regular file OR a directory at this path.
    let exists = files.contains_key(value) || snap.dirs.iter().any(|d| d == value);
    CheckResult {
        check: check.to_string(),
        passed: exists,
        detail: if exists {
            "exists".to_string()
        } else {
            "not found".to_string()
        },
        weight,
    }
}

fn check_dir_exists(check: &str, weight: f64, value: &str, snap: &Snapshot) -> CheckResult {
    let is_dir = snap.dirs.iter().any(|d| d == value);
    CheckResult {
        check: check.to_string(),
        passed: is_dir,
        detail: if is_dir {
            "directory exists".to_string()
        } else {
            "directory not found".to_string()
        },
        weight,
    }
}

fn check_file_contains(
    check: &str,
    weight: f64,
    value: &str,
    files: &BTreeMap<String, String>,
) -> CheckResult {
    // Format: "file_contains:/path:expected_text" -> value is "/path:text".
    let (path_str, text) = match value.split_once(':') {
        Some((p, t)) => (p, t),
        None => {
            return CheckResult {
                check: check.to_string(),
                passed: false,
                detail: "invalid format, expected file_contains:/path:text".to_string(),
                weight,
            };
        }
    };

    match files.get(path_str) {
        Some(content) => {
            let found = content.contains(text);
            CheckResult {
                check: check.to_string(),
                passed: found,
                detail: if found {
                    "found in file".to_string()
                } else {
                    format!("'{}' not found in {}", text, path_str)
                },
                weight,
            }
        }
        None => CheckResult {
            check: check.to_string(),
            passed: false,
            detail: format!("cannot read {}", path_str),
            weight,
        },
    }
}

fn check_file_not_contains(
    check: &str,
    weight: f64,
    value: &str,
    files: &BTreeMap<String, String>,
) -> CheckResult {
    // Format: "file_not_contains:/path:text". A missing file fails: the check
    // asserts an edit to a file that must still exist, not its deletion.
    let Some((path_str, text)) = value.split_once(':') else {
        return CheckResult {
            check: check.to_string(),
            passed: false,
            detail: "invalid format, expected file_not_contains:/path:text".to_string(),
            weight,
        };
    };
    let (passed, detail) = match files.get(path_str) {
        Some(content) if content.contains(text) => {
            (false, format!("'{}' still present in {}", text, path_str))
        }
        Some(_) => (true, "absent from file".to_string()),
        None => (false, format!("cannot read {}", path_str)),
    };
    CheckResult {
        check: check.to_string(),
        passed,
        detail,
        weight,
    }
}

fn check_file_line_regex(
    check: &str,
    weight: f64,
    value: &str,
    files: &BTreeMap<String, String>,
) -> CheckResult {
    // Format: "file_line_regex:/path:pattern". Match is scoped to one line so
    // CSV/table row expectations cannot pass from unrelated substrings.
    let (path_str, pattern) = match value.split_once(':') {
        Some((p, t)) => (p, t),
        None => {
            return CheckResult {
                check: check.to_string(),
                passed: false,
                detail: "invalid format, expected file_line_regex:/path:pattern".to_string(),
                weight,
            };
        }
    };

    let re = match regex::Regex::new(pattern) {
        Ok(re) => re,
        Err(e) => {
            return CheckResult {
                check: check.to_string(),
                passed: false,
                detail: format!("invalid regex: {}", e),
                weight,
            };
        }
    };

    match files.get(path_str) {
        Some(content) => {
            let found = content.lines().any(|line| re.is_match(line));
            CheckResult {
                check: check.to_string(),
                passed: found,
                detail: if found {
                    "matched file line".to_string()
                } else {
                    format!(
                        "pattern '{}' not matched by any line in {}",
                        pattern, path_str
                    )
                },
                weight,
            }
        }
        None => CheckResult {
            check: check.to_string(),
            passed: false,
            detail: format!("cannot read {}", path_str),
            weight,
        },
    }
}

/// Lexically resolve `target` against the directory holding `link`
/// (`.`/`..` folded, no filesystem access), the way `readlink -m` would.
fn resolve_link_target(link: &str, target: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        let parent = link.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        parts.extend(parent.split('/').filter(|p| !p.is_empty()));
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

fn check_symlink(check: &str, weight: f64, value: &str, snap: &Snapshot) -> CheckResult {
    // Format: "symlink:/link/path:target".
    let Some((link, expected)) = value.split_once(':') else {
        return CheckResult {
            check: check.to_string(),
            passed: false,
            detail: "invalid format, expected symlink:/path:target".to_string(),
            weight,
        };
    };
    let (passed, detail) = match snap.links.get(link) {
        None => (false, format!("{link} is not a symlink")),
        Some(actual) => {
            let ok = resolve_link_target(link, actual) == resolve_link_target(link, expected);
            (ok, format!("{link} -> {actual}"))
        }
    };
    CheckResult {
        check: check.to_string(),
        passed,
        detail,
        weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::ToolOutput;

    fn snap_with(outputs: Vec<ToolOutput>) -> Snapshot {
        let last_exit_code = outputs.last().map(|t| t.exit_code);
        Snapshot {
            tool_outputs: outputs,
            last_exit_code,
            dirs: Vec::new(),
            links: BTreeMap::new(),
        }
    }

    fn files_with(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn exit_code_pass() {
        let snap = snap_with(vec![ToolOutput {
            commands: "echo hi".into(),
            stdout: "hi\n".into(),
            stderr: String::new(),
            exit_code: 0,
        }]);
        let r = check_exit_code("exit_code:0", 1.0, "0", &snap);
        assert!(r.passed);
    }

    #[test]
    fn exit_code_fail() {
        let snap = snap_with(vec![ToolOutput {
            commands: "false".into(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 1,
        }]);
        let r = check_exit_code("exit_code:0", 1.0, "0", &snap);
        assert!(!r.passed);
    }

    #[test]
    fn exit_code_no_tool_call_is_minus_one() {
        let snap = snap_with(vec![]);
        let r = check_exit_code("exit_code:0", 1.0, "0", &snap);
        assert!(!r.passed);
    }

    #[test]
    fn stdout_contains_pass() {
        let snap = snap_with(vec![ToolOutput {
            commands: "echo hello world".into(),
            stdout: "hello world\n".into(),
            stderr: String::new(),
            exit_code: 0,
        }]);
        let r = check_stdout_contains("stdout_contains:hello", 1.0, "hello", &snap);
        assert!(r.passed);
    }

    #[test]
    fn dir_exists_pass_and_fail() {
        let snap = Snapshot {
            tool_outputs: vec![],
            last_exit_code: Some(0),
            dirs: vec!["/project/src".to_string()],
            links: BTreeMap::new(),
        };
        assert!(check_dir_exists("dir_exists:/project/src", 1.0, "/project/src", &snap).passed);
        assert!(!check_dir_exists("dir_exists:/project/x", 1.0, "/project/x", &snap).passed);
    }

    #[test]
    fn file_exists_matches_file_or_dir() {
        let snap = Snapshot {
            tool_outputs: vec![],
            last_exit_code: Some(0),
            dirs: vec!["/a/dir".to_string()],
            links: BTreeMap::new(),
        };
        let files = files_with(&[("/a/file.txt", "x")]);
        assert!(
            check_file_exists("file_exists:/a/file.txt", 1.0, "/a/file.txt", &snap, &files).passed
        );
        assert!(check_file_exists("file_exists:/a/dir", 1.0, "/a/dir", &snap, &files).passed);
        assert!(!check_file_exists("file_exists:/nope", 1.0, "/nope", &snap, &files).passed);
    }

    #[test]
    fn file_contains_pass_and_fail() {
        let files = files_with(&[("/data/config.yaml", "version: 1\nupdated: true\n")]);
        assert!(
            check_file_contains(
                "file_contains:/data/config.yaml:updated: true",
                1.0,
                "/data/config.yaml:updated: true",
                &files,
            )
            .passed
        );
        assert!(
            !check_file_contains(
                "file_contains:/data/config.yaml:missing",
                1.0,
                "/data/config.yaml:missing",
                &files,
            )
            .passed
        );
    }

    #[test]
    fn file_line_regex_matches_quoted_or_unquoted_csv_row() {
        let files = files_with(&[(
            "/data/employees.csv",
            "name,department,salary\nAlice Chen,Engineering,120000\n\"Bob Park\",\"Marketing\",95000\n",
        )]);
        let unquoted = check_file_line_regex(
            r#"file_line_regex:/data/employees.csv:^(?:"Alice Chen"|Alice Chen),(?:"Engineering"|Engineering),(?:"120000"|120000)$"#,
            1.0,
            r#"/data/employees.csv:^(?:"Alice Chen"|Alice Chen),(?:"Engineering"|Engineering),(?:"120000"|120000)$"#,
            &files,
        );
        let quoted = check_file_line_regex(
            r#"file_line_regex:/data/employees.csv:^(?:"Bob Park"|Bob Park),(?:"Marketing"|Marketing),(?:"95000"|95000)$"#,
            1.0,
            r#"/data/employees.csv:^(?:"Bob Park"|Bob Park),(?:"Marketing"|Marketing),(?:"95000"|95000)$"#,
            &files,
        );
        assert!(unquoted.passed);
        assert!(quoted.passed);
    }

    #[test]
    fn file_line_regex_rejects_values_split_across_lines() {
        let files = files_with(&[(
            "/data/employees.csv",
            "name,department,salary\nAlice Chen\nEngineering\n120000\n",
        )]);
        let result = check_file_line_regex(
            r#"file_line_regex:/data/employees.csv:^(?:"Alice Chen"|Alice Chen),(?:"Engineering"|Engineering),(?:"120000"|120000)$"#,
            1.0,
            r#"/data/employees.csv:^(?:"Alice Chen"|Alice Chen),(?:"Engineering"|Engineering),(?:"120000"|120000)$"#,
            &files,
        );
        assert!(!result.passed);
    }

    #[test]
    fn json_to_csv_export_regexes_reject_unbalanced_quotes() {
        let files = files_with(&[(
            "/data/employees.csv",
            "name,department,salary\n\"Alice Chen,Engineering,120000\nBob Park\",Marketing,95000\n\"Carol Wu,Engineering,115000\nDave Kim\",Sales,88000\n",
        )]);
        let task = include_str!("../data/eval-tasks.jsonl")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|task| task["id"] == "json_to_csv_export")
            .unwrap();
        let mut checks = task["expectations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|exp| {
                exp["check"]
                    .as_str()
                    .filter(|check| check.starts_with("file_line_regex:/data/employees.csv:"))
            })
            .peekable();

        assert!(
            checks.peek().is_some(),
            "expected at least one file_line_regex check; task schema may have changed"
        );

        for check in checks {
            let value = check.strip_prefix("file_line_regex:").unwrap();
            let result = check_file_line_regex(check, 1.0, value, &files);
            assert!(!result.passed, "malformed CSV matched check: {check}");
        }
    }

    #[test]
    fn file_not_contains_pass_fail_and_missing() {
        let files = files_with(&[("/d/eu.csv", "order,amount\nB1,50\n")]);
        let ok = "/d/eu.csv:n/a";
        assert!(check_file_not_contains("c", 1.0, ok, &files).passed);
        let present = "/d/eu.csv:B1,50";
        assert!(!check_file_not_contains("c", 1.0, present, &files).passed);
        let missing = "/d/gone.csv:n/a";
        assert!(!check_file_not_contains("c", 1.0, missing, &files).passed);
    }

    #[test]
    fn symlink_accepts_relative_or_absolute_target() {
        let mut snap = snap_with(vec![]);
        snap.links
            .insert("/srv/app/current".into(), "releases/v2".into());
        let v = "/srv/app/current:releases/v2";
        assert!(check_symlink("symlink", 1.0, v, &snap).passed);
        let v = "/srv/app/current:/srv/app/releases/v2";
        assert!(check_symlink("symlink", 1.0, v, &snap).passed);
        let v = "/srv/app/current:./releases/v2/";
        assert!(check_symlink("symlink", 1.0, v, &snap).passed);
    }

    #[test]
    fn symlink_rejects_wrong_target_or_non_link() {
        let mut snap = snap_with(vec![]);
        snap.links
            .insert("/srv/app/current".into(), "releases/v1".into());
        let v = "/srv/app/current:releases/v2";
        assert!(!check_symlink("symlink", 1.0, v, &snap).passed);
        // A copied directory (no symlink recorded) must not pass.
        let v = "/srv/app/other:releases/v2";
        assert!(!check_symlink("symlink", 1.0, v, &snap).passed);
    }

    #[test]
    fn evaluate_aggregates_weighted_rate_and_all_passed() {
        let snap = snap_with(vec![ToolOutput {
            commands: "echo hi".into(),
            stdout: "hi\n".into(),
            stderr: String::new(),
            exit_code: 0,
        }]);
        let files = BTreeMap::new();
        let exps = vec![
            ("stdout_contains:hi".to_string(), 1.0),
            ("exit_code:0".to_string(), 1.0),
            ("stdout_contains:nope".to_string(), 2.0),
        ];
        let summary = evaluate(&exps, &snap, &files);
        assert!(!summary.all_passed());
        assert_eq!(summary.score, 2.0);
        assert_eq!(summary.max_score, 4.0);
        assert_eq!(summary.rate(), 0.5);
    }
}
