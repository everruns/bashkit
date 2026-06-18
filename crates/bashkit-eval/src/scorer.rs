// Scorer: deterministic checks against agent trace + VFS
// Parses check strings like "exit_code:0", "stdout_contains:hello"
// See specs/eval.md for check type reference

use std::path::Path;

use bashkit::{FileSystem, FileType};
use serde::{Deserialize, Serialize};

use crate::agent::AgentTrace;
use crate::dataset::Expectation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreResult {
    pub check: String,
    pub passed: bool,
    pub detail: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskScore {
    pub task_id: String,
    pub results: Vec<ScoreResult>,
    /// Weighted sum of passed checks
    pub score: f64,
    /// Sum of all weights
    pub max_score: f64,
}

impl TaskScore {
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }

    pub fn rate(&self) -> f64 {
        if self.max_score == 0.0 {
            1.0
        } else {
            self.score / self.max_score
        }
    }
}

/// Score a single task's trace against its expectations
pub async fn score_task(
    task_id: &str,
    trace: &AgentTrace,
    fs: &dyn FileSystem,
    expectations: &[Expectation],
) -> TaskScore {
    let mut results = Vec::new();

    for exp in expectations {
        let result = evaluate_check(&exp.check, exp.weight, trace, fs).await;
        results.push(result);
    }

    let max_score: f64 = results.iter().map(|r| r.weight).sum();
    let score: f64 = results.iter().filter(|r| r.passed).map(|r| r.weight).sum();

    TaskScore {
        task_id: task_id.to_string(),
        results,
        score,
        max_score,
    }
}

async fn evaluate_check(
    check: &str,
    weight: f64,
    trace: &AgentTrace,
    fs: &dyn FileSystem,
) -> ScoreResult {
    let (check_type, check_value) = check.split_once(':').unwrap_or((check, ""));

    match check_type {
        "exit_code" => check_exit_code(check, weight, check_value, trace),
        "stdout_contains" => check_stdout_contains(check, weight, check_value, trace),
        "stdout_regex" => check_stdout_regex(check, weight, check_value, trace),
        "stderr_empty" => check_stderr_empty(check, weight, trace),
        "file_exists" => check_file_exists(check, weight, check_value, fs).await,
        "dir_exists" => check_dir_exists(check, weight, check_value, fs).await,
        "file_contains" => check_file_contains(check, weight, check_value, fs).await,
        "file_line_regex" => check_file_line_regex(check, weight, check_value, fs).await,
        "llm_judge" => ScoreResult {
            check: check.to_string(),
            passed: true,
            detail: "llm_judge not implemented (stub, weight=0)".to_string(),
            weight: 0.0,
        },
        _ => ScoreResult {
            check: check.to_string(),
            passed: false,
            detail: format!("unknown check type: {}", check_type),
            weight,
        },
    }
}

fn check_exit_code(check: &str, weight: f64, value: &str, trace: &AgentTrace) -> ScoreResult {
    let expected: i32 = value.parse().unwrap_or(0);
    let actual = trace
        .last_tool_response
        .as_ref()
        .map(|r| r.exit_code)
        .unwrap_or(-1);
    ScoreResult {
        check: check.to_string(),
        passed: actual == expected,
        detail: format!("expected {}, got {}", expected, actual),
        weight,
    }
}

fn check_stdout_contains(check: &str, weight: f64, value: &str, trace: &AgentTrace) -> ScoreResult {
    let found = trace.tool_calls.iter().any(|tc| tc.stdout.contains(value));
    ScoreResult {
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

fn check_stdout_regex(check: &str, weight: f64, value: &str, trace: &AgentTrace) -> ScoreResult {
    match regex::Regex::new(value) {
        Ok(re) => {
            let found = trace.tool_calls.iter().any(|tc| re.is_match(&tc.stdout));
            ScoreResult {
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
        Err(e) => ScoreResult {
            check: check.to_string(),
            passed: false,
            detail: format!("invalid regex: {}", e),
            weight,
        },
    }
}

fn check_stderr_empty(check: &str, weight: f64, trace: &AgentTrace) -> ScoreResult {
    let all_empty = trace.tool_calls.iter().all(|tc| tc.stderr.is_empty());
    ScoreResult {
        check: check.to_string(),
        passed: all_empty,
        detail: if all_empty {
            "all stderr empty".to_string()
        } else {
            let first_stderr = trace
                .tool_calls
                .iter()
                .find(|tc| !tc.stderr.is_empty())
                .map(|tc| tc.stderr.clone())
                .unwrap_or_default();
            format!(
                "stderr: {}",
                first_stderr.chars().take(100).collect::<String>()
            )
        },
        weight,
    }
}

async fn check_file_exists(
    check: &str,
    weight: f64,
    value: &str,
    fs: &dyn FileSystem,
) -> ScoreResult {
    let path = Path::new(value);
    let exists = fs.stat(path).await.is_ok();
    ScoreResult {
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

async fn check_dir_exists(
    check: &str,
    weight: f64,
    value: &str,
    fs: &dyn FileSystem,
) -> ScoreResult {
    let path = Path::new(value);
    let is_dir = fs
        .stat(path)
        .await
        .map(|m| m.file_type == FileType::Directory)
        .unwrap_or(false);
    ScoreResult {
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

async fn check_file_contains(
    check: &str,
    weight: f64,
    value: &str,
    fs: &dyn FileSystem,
) -> ScoreResult {
    // Format: "file_contains:/path:expected_text"
    // value is everything after "file_contains:", so "/path:text"
    let (path_str, text) = match value.split_once(':') {
        Some((p, t)) => (p, t),
        None => {
            return ScoreResult {
                check: check.to_string(),
                passed: false,
                detail: "invalid format, expected file_contains:/path:text".to_string(),
                weight,
            };
        }
    };

    let path = Path::new(path_str);
    match fs.read_file(path).await {
        Ok(bytes) => {
            let content = String::from_utf8_lossy(&bytes);
            let found = content.contains(text);
            ScoreResult {
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
        Err(e) => ScoreResult {
            check: check.to_string(),
            passed: false,
            detail: format!("cannot read {}: {}", path_str, e),
            weight,
        },
    }
}

async fn check_file_line_regex(
    check: &str,
    weight: f64,
    value: &str,
    fs: &dyn FileSystem,
) -> ScoreResult {
    // Format: "file_line_regex:/path:pattern". Match is scoped to one line so
    // CSV/table row expectations cannot pass from unrelated substrings.
    let (path_str, pattern) = match value.split_once(':') {
        Some((p, t)) => (p, t),
        None => {
            return ScoreResult {
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
            return ScoreResult {
                check: check.to_string(),
                passed: false,
                detail: format!("invalid regex: {}", e),
                weight,
            };
        }
    };

    let path = Path::new(path_str);
    match fs.read_file(path).await {
        Ok(bytes) => {
            let content = String::from_utf8_lossy(&bytes);
            let found = content.lines().any(|line| re.is_match(line));
            ScoreResult {
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
        Err(e) => ScoreResult {
            check: check.to_string(),
            passed: false,
            detail: format!("cannot read {}: {}", path_str, e),
            weight,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ToolCallResult;
    use bashkit::{FileSystem, InMemoryFs};
    use std::path::Path;

    fn make_trace(tool_calls: Vec<ToolCallResult>) -> AgentTrace {
        let last = tool_calls.last().cloned();
        let count = tool_calls.len();
        AgentTrace {
            messages: vec![],
            tool_call_count: count,
            turns: 1,
            tool_calls,
            last_tool_response: last,
            natural_stop: true,
            total_input_tokens: 0,
            total_output_tokens: 0,
            duration_ms: 0,
        }
    }

    #[test]
    fn exit_code_pass() {
        let trace = make_trace(vec![ToolCallResult {
            commands: "echo hi".into(),
            stdout: "hi\n".into(),
            stderr: String::new(),
            exit_code: 0,
        }]);
        let r = check_exit_code("exit_code:0", 1.0, "0", &trace);
        assert!(r.passed);
    }

    #[test]
    fn exit_code_fail() {
        let trace = make_trace(vec![ToolCallResult {
            commands: "false".into(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 1,
        }]);
        let r = check_exit_code("exit_code:0", 1.0, "0", &trace);
        assert!(!r.passed);
    }

    #[test]
    fn stdout_contains_pass() {
        let trace = make_trace(vec![ToolCallResult {
            commands: "echo hello world".into(),
            stdout: "hello world\n".into(),
            stderr: String::new(),
            exit_code: 0,
        }]);
        let r = check_stdout_contains("stdout_contains:hello", 1.0, "hello", &trace);
        assert!(r.passed);
    }

    #[tokio::test]
    async fn file_line_regex_matches_quoted_or_unquoted_csv_row() {
        let fs = InMemoryFs::new();
        fs.mkdir(Path::new("/data"), false).await.unwrap();
        fs.write_file(
            Path::new("/data/employees.csv"),
            b"name,department,salary\nAlice Chen,Engineering,120000\n\"Bob Park\",\"Marketing\",95000\n",
        )
        .await
        .unwrap();

        let unquoted = check_file_line_regex(
            r#"file_line_regex:/data/employees.csv:^(?:\"Alice Chen\"|Alice Chen),(?:\"Engineering\"|Engineering),(?:\"120000\"|120000)$"#,
            1.0,
            r#"/data/employees.csv:^(?:\"Alice Chen\"|Alice Chen),(?:\"Engineering\"|Engineering),(?:\"120000\"|120000)$"#,
            &fs,
        )
        .await;
        let quoted = check_file_line_regex(
            r#"file_line_regex:/data/employees.csv:^(?:\"Bob Park\"|Bob Park),(?:\"Marketing\"|Marketing),(?:\"95000\"|95000)$"#,
            1.0,
            r#"/data/employees.csv:^(?:\"Bob Park\"|Bob Park),(?:\"Marketing\"|Marketing),(?:\"95000\"|95000)$"#,
            &fs,
        )
        .await;

        assert!(unquoted.passed);
        assert!(quoted.passed);
    }

    #[tokio::test]
    async fn file_line_regex_rejects_values_split_across_lines() {
        let fs = InMemoryFs::new();
        fs.mkdir(Path::new("/data"), false).await.unwrap();
        fs.write_file(
            Path::new("/data/employees.csv"),
            b"name,department,salary\nAlice Chen\nEngineering\n120000\n",
        )
        .await
        .unwrap();

        let result = check_file_line_regex(
            r#"file_line_regex:/data/employees.csv:^(?:\"Alice Chen\"|Alice Chen),(?:\"Engineering\"|Engineering),(?:\"120000\"|120000)$"#,
            1.0,
            r#"/data/employees.csv:^(?:\"Alice Chen\"|Alice Chen),(?:\"Engineering\"|Engineering),(?:\"120000\"|120000)$"#,
            &fs,
        )
        .await;

        assert!(!result.passed);
    }

    #[tokio::test]
    async fn json_to_csv_export_regexes_reject_unbalanced_quotes() {
        let fs = InMemoryFs::new();
        fs.mkdir(Path::new("/data"), false).await.unwrap();
        fs.write_file(
            Path::new("/data/employees.csv"),
            b"name,department,salary\n\"Alice Chen,Engineering,120000\nBob Park\",Marketing,95000\n\"Carol Wu,Engineering,115000\nDave Kim\",Sales,88000\n",
        )
        .await
        .unwrap();

        let task = include_str!("../data/eval-tasks.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|task| task["id"] == "json_to_csv_export")
            .unwrap();
        let checks = task["expectations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|exp| {
                exp["check"]
                    .as_str()
                    .filter(|check| check.starts_with("file_line_regex:/data/employees.csv:"))
            });

        for check in checks {
            let value = check.strip_prefix("file_line_regex:").unwrap();
            let result = check_file_line_regex(check, 1.0, value, &fs).await;
            assert!(!result.passed, "malformed CSV matched check: {check}");
        }
    }
}
