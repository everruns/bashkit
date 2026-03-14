// Dataset types for scripting tool evals
// Each task defines mock tools + a prompt. The agent uses ScriptedTool or
// individual tools (baseline mode) to accomplish the task.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::dataset::Expectation;

/// Mock tool definition in a scripting eval task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockToolDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub schema: serde_json::Value,
    /// Tags for discover --tag filtering.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Category for discover --category filtering.
    #[serde(default)]
    pub category: Option<String>,
    /// Mock response behavior.
    pub mock: MockBehavior,
}

/// How a mock tool generates responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MockBehavior {
    /// Always return this string.
    Static(String),
    /// Return based on a parameter value, with optional default.
    ByParam {
        param: String,
        responses: HashMap<String, String>,
        #[serde(default)]
        default: Option<String>,
    },
}

/// A scripting tool eval task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptingEvalTask {
    pub id: String,
    pub category: String,
    pub description: String,
    #[serde(default)]
    pub system: Option<String>,
    pub prompt: String,
    pub tools: Vec<MockToolDef>,
    #[serde(default)]
    pub files: HashMap<String, String>,
    /// Use ScriptingToolSet with WithDiscovery mode: tool names hidden from
    /// system prompt, LLM must use `discover` and `help` builtins.
    #[serde(default)]
    pub discovery_mode: bool,
    pub expectations: Vec<Expectation>,
}

/// Load scripting eval tasks from JSONL.
pub fn load_scripting_dataset(path: &str) -> Result<Vec<ScriptingEvalTask>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read dataset: {}", path))?;

    let mut tasks = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let task: ScriptingEvalTask = serde_json::from_str(line)
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
    fn parse_static_mock() {
        let json = r#"{"id":"t1","category":"test","description":"d","prompt":"p","tools":[{"name":"get_user","description":"Fetch user","schema":{"type":"object","properties":{"id":{"type":"integer"}}},"mock":"user data"}],"expectations":[{"check":"exit_code:0"}]}"#;
        let task: ScriptingEvalTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.tools.len(), 1);
        assert!(matches!(&task.tools[0].mock, MockBehavior::Static(s) if s == "user data"));
    }

    #[test]
    fn parse_by_param_mock() {
        let json = r#"{"id":"t2","category":"test","description":"d","prompt":"p","tools":[{"name":"get_page","description":"Get page","schema":{},"mock":{"param":"page","responses":{"1":"page1","2":"page2"},"default":"empty"}}],"expectations":[{"check":"exit_code:0"}]}"#;
        let task: ScriptingEvalTask = serde_json::from_str(json).unwrap();
        match &task.tools[0].mock {
            MockBehavior::ByParam {
                param,
                responses,
                default,
            } => {
                assert_eq!(param, "page");
                assert_eq!(responses.len(), 2);
                assert_eq!(default.as_deref(), Some("empty"));
            }
            _ => panic!("expected ByParam"),
        }
    }

    #[test]
    fn parse_tags_category_discovery() {
        let json = r#"{"id":"t3","category":"discovery","description":"d","prompt":"p","discovery_mode":true,"tools":[{"name":"get_user","description":"Fetch user","schema":{},"tags":["read","users"],"category":"users","mock":"ok"}],"expectations":[{"check":"exit_code:0"}]}"#;
        let task: ScriptingEvalTask = serde_json::from_str(json).unwrap();
        assert!(task.discovery_mode);
        assert_eq!(task.tools[0].tags, vec!["read", "users"]);
        assert_eq!(task.tools[0].category.as_deref(), Some("users"));
    }

    #[test]
    fn parse_discovery_dataset() {
        let tasks = load_scripting_dataset("data/scripting-tool/discovery.jsonl").unwrap();
        assert_eq!(tasks.len(), 4);
        assert!(tasks.iter().all(|t| t.discovery_mode));
        assert!(tasks.iter().all(|t| t.category == "discovery"));
        // All tools should have categories
        for task in &tasks {
            for tool in &task.tools {
                assert!(
                    tool.category.is_some(),
                    "tool {} in task {} missing category",
                    tool.name,
                    task.id
                );
            }
        }
    }
}
