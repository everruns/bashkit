// Dataset types and JSONL loader
// Each line in the JSONL file is one EvalTask
// See specs/012-eval.md for format specification

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single evaluation task loaded from JSONL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalTask {
    pub id: String,
    pub category: String,
    pub description: String,
    /// System message override. None = use BashTool default.
    #[serde(default)]
    pub system: Option<String>,
    pub prompt: String,
    /// Files to pre-populate in VFS. Key: absolute path, Value: content.
    #[serde(default)]
    pub files: HashMap<String, String>,
    pub expectations: Vec<Expectation>,
}

/// A single check to run against the agent trace/VFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expectation {
    /// Check string like "exit_code:0", "file_exists:/path"
    pub check: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

/// Load tasks from a JSONL file (one JSON object per line)
pub fn load_dataset(path: &str) -> Result<Vec<EvalTask>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read dataset: {}", path))?;

    let mut tasks = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let task: EvalTask = serde_json::from_str(line)
            .with_context(|| format!("failed to parse line {} in {}", i + 1, path))?;
        tasks.push(task);
    }

    anyhow::ensure!(!tasks.is_empty(), "dataset is empty: {}", path);
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_task_minimal() {
        let json = r#"{"id":"t1","category":"test","description":"desc","prompt":"do it","expectations":[{"check":"exit_code:0"}]}"#;
        let task: EvalTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.id, "t1");
        assert!(task.files.is_empty());
        assert!(task.system.is_none());
        assert_eq!(task.expectations[0].weight, 1.0);
    }

    #[test]
    fn parse_task_with_files() {
        let json = r#"{"id":"t2","category":"test","description":"desc","prompt":"do it","files":{"/data/x.txt":"hello"},"expectations":[{"check":"file_exists:/data/x.txt","weight":2.0}]}"#;
        let task: EvalTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.files.get("/data/x.txt").unwrap(), "hello");
        assert_eq!(task.expectations[0].weight, 2.0);
    }
}
