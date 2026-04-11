//! MCP (Model Context Protocol) server implementation
//!
//! Implements a JSON-RPC 2.0 server that exposes bashkit as an MCP tool.
//! Optionally registers ScriptedTool instances as additional MCP tools.
//!
//! Protocol:
//! - Input: JSON-RPC requests on stdin
//! - Output: JSON-RPC responses on stdout

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

/// JSON-RPC 2.0 request
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

/// MCP tool definition
#[derive(Debug, Serialize)]
struct McpTool {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: serde_json::Value,
}

/// MCP server capabilities
#[derive(Debug, Serialize)]
struct ServerCapabilities {
    tools: serde_json::Value,
}

/// MCP server info
#[derive(Debug, Serialize)]
struct ServerInfo {
    name: String,
    version: String,
}

/// MCP initialize result
#[derive(Debug, Serialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
}

/// Tool call arguments for bash execution
#[derive(Debug, Deserialize)]
struct BashToolArgs {
    script: String,
}

/// Tool call result
#[derive(Debug, Serialize)]
struct ToolResult {
    content: Vec<ContentItem>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ContentItem {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

/// MCP server with optional ScriptedTool registrations.
///
/// Accepts a factory function that produces configured `Bash` instances,
/// ensuring CLI execution limits (max_commands, etc.) are applied to every
/// MCP `tools/call` invocation.
///
/// DESIGN: Session-level counters (session_commands, session_exec_calls) are
/// tracked cumulatively here because each tools/call creates a fresh Bash
/// instance. Without this, an LLM agent could make unlimited sequential calls
/// without session limits ever triggering. See issue #1193.
pub struct McpServer {
    bash_factory: Box<dyn Fn() -> bashkit::Bash + Send>,
    /// Cumulative session command count across all MCP tool calls.
    cumulative_commands: u64,
    /// Cumulative session exec call count across all MCP tool calls.
    cumulative_exec_calls: u64,
    #[cfg(feature = "scripted_tool")]
    scripted_tools: Vec<bashkit::ScriptedTool>,
}

impl McpServer {
    /// Create a new MCP server with only the default `bash` tool.
    /// Each `tools/call` will create a `Bash` via the provided factory,
    /// inheriting whatever limits/configuration the caller sets up.
    pub fn new(bash_factory: impl Fn() -> bashkit::Bash + Send + 'static) -> Self {
        Self {
            bash_factory: Box::new(bash_factory),
            cumulative_commands: 0,
            cumulative_exec_calls: 0,
            #[cfg(feature = "scripted_tool")]
            scripted_tools: Vec::new(),
        }
    }

    /// Register a ScriptedTool. It will appear in `tools/list` and route
    /// `tools/call` to `ScriptedTool::execute()`.
    #[cfg(feature = "scripted_tool")]
    #[allow(dead_code)] // Public API for external consumers; used in tests
    pub fn register_scripted_tool(&mut self, tool: bashkit::ScriptedTool) {
        self.scripted_tools.push(tool);
    }

    /// Run the server, reading JSON-RPC from stdin and writing responses to stdout.
    pub async fn run(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();

        for line in stdin.lock().lines() {
            let line = line.context("Failed to read line from stdin")?;
            if line.trim().is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(req) => req,
                Err(e) => {
                    eprintln!("MCP parse error detail: {}", e);
                    let response = JsonRpcResponse::error(
                        serde_json::Value::Null,
                        -32700,
                        "Parse error".to_string(),
                    );
                    writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                    stdout.flush()?;
                    continue;
                }
            };

            let response = self.handle_request(request).await;
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }

        Ok(())
    }

    async fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => Self::handle_initialize(request.id),
            "initialized" => JsonRpcResponse::success(request.id, serde_json::Value::Null),
            "tools/list" => self.handle_tools_list(request.id),
            "tools/call" => self.handle_tools_call(request.id, request.params).await,
            "shutdown" => JsonRpcResponse::success(request.id, serde_json::Value::Null),
            _ => JsonRpcResponse::error(request.id, -32601, "Method not found".to_string()),
        }
    }

    fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: serde_json::json!({}),
            },
            server_info: ServerInfo {
                name: "bashkit".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        JsonRpcResponse::success(id, serde_json::to_value(result).expect("serialize init"))
    }

    fn handle_tools_list(&self, id: serde_json::Value) -> JsonRpcResponse {
        #[allow(unused_mut)]
        let mut tools = vec![McpTool {
            name: "bash".to_string(),
            description: "Execute a bash script in a virtual environment".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "script": {
                        "type": "string",
                        "description": "The bash script to execute"
                    }
                },
                "required": ["script"]
            }),
        }];

        #[cfg(feature = "scripted_tool")]
        {
            use bashkit::tool::Tool;
            for st in &self.scripted_tools {
                tools.push(McpTool {
                    name: st.name().to_string(),
                    description: st.short_description().to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "commands": {
                                "type": "string",
                                "description": st.description()
                            }
                        },
                        "required": ["commands"]
                    }),
                });
            }
        }

        JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
    }

    async fn handle_tools_call(
        &mut self,
        id: serde_json::Value,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or_default();

        #[cfg(feature = "scripted_tool")]
        {
            if let Some(st) = self.scripted_tools.iter_mut().find(|t| {
                use bashkit::tool::Tool;
                t.name() == tool_name
            }) {
                return Self::handle_scripted_tool_call(id, st, arguments).await;
            }
        }

        if tool_name != "bash" {
            return JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name));
        }

        let args: BashToolArgs = match serde_json::from_value(arguments) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("MCP invalid params detail: {}", e);
                return JsonRpcResponse::error(id, -32602, "Invalid parameters".to_string());
            }
        };

        let mut bash = (self.bash_factory)();
        // Restore cumulative session counters so limits persist across MCP calls
        bash.restore_session_counters(self.cumulative_commands, self.cumulative_exec_calls);
        let result = match bash.exec(&args.script).await {
            Ok(r) => r,
            Err(e) => {
                // Update cumulative counters even on error (commands were still counted)
                let (cmds, execs) = bash.session_counters();
                self.cumulative_commands = cmds;
                self.cumulative_exec_calls = execs;
                eprintln!("MCP execution error detail: {}", e);
                let tool_result = ToolResult {
                    content: vec![ContentItem {
                        content_type: "text".to_string(),
                        text: "Execution failed".to_string(),
                    }],
                    is_error: Some(true),
                };
                return JsonRpcResponse::success(
                    id,
                    serde_json::to_value(tool_result).expect("serialize"),
                );
            }
        };
        // Update cumulative counters after successful execution
        let (cmds, execs) = bash.session_counters();
        self.cumulative_commands = cmds;
        self.cumulative_exec_calls = execs;

        let mut output = result.stdout;
        if !result.stderr.is_empty() {
            output.push_str("\n[stderr]\n");
            output.push_str(&result.stderr);
        }
        if result.exit_code != 0 {
            output.push_str(&format!("\n[exit code: {}]", result.exit_code));
        }

        let tool_result = ToolResult {
            content: vec![ContentItem {
                content_type: "text".to_string(),
                text: output,
            }],
            is_error: if result.exit_code != 0 {
                Some(true)
            } else {
                None
            },
        };

        JsonRpcResponse::success(id, serde_json::to_value(tool_result).expect("serialize"))
    }

    #[cfg(feature = "scripted_tool")]
    async fn handle_scripted_tool_call(
        id: serde_json::Value,
        tool: &mut bashkit::ScriptedTool,
        arguments: serde_json::Value,
    ) -> JsonRpcResponse {
        use bashkit::tool::{Tool, ToolRequest};

        let commands = arguments
            .get("commands")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let resp = tool
            .execute(ToolRequest {
                commands: commands.to_string(),
                timeout_ms: None,
            })
            .await;

        let mut output = resp.stdout;
        if !resp.stderr.is_empty() {
            output.push_str("\n[stderr]\n");
            output.push_str(&resp.stderr);
        }
        if resp.exit_code != 0 {
            output.push_str(&format!("\n[exit code: {}]", resp.exit_code));
        }

        let tool_result = ToolResult {
            content: vec![ContentItem {
                content_type: "text".to_string(),
                text: output,
            }],
            is_error: if resp.exit_code != 0 {
                Some(true)
            } else {
                None
            },
        };

        JsonRpcResponse::success(id, serde_json::to_value(tool_result).expect("serialize"))
    }
}

/// Run the MCP server with a factory that produces configured `Bash` instances.
pub async fn run(bash_factory: impl Fn() -> bashkit::Bash + Send + 'static) -> Result<()> {
    let mut server = McpServer::new(bash_factory);
    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_initialize() {
        let mut server = McpServer::new(bashkit::Bash::new);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "initialize".to_string(),
            params: serde_json::json!({}),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "bashkit");
    }

    #[tokio::test]
    async fn test_tools_list_default() {
        let mut server = McpServer::new(bashkit::Bash::new);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/list".to_string(),
            params: serde_json::json!({}),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        let tools = result["tools"].as_array().expect("tools array");
        assert!(tools.iter().any(|t| t["name"] == "bash"));
    }

    #[tokio::test]
    async fn test_tools_call_bash() {
        let mut server = McpServer::new(bashkit::Bash::new);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "script": "echo hello" }
            }),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        let text = result["content"][0]["text"].as_str().expect("text");
        assert!(text.contains("hello"));
    }

    #[tokio::test]
    async fn test_tools_call_unknown() {
        let mut server = McpServer::new(bashkit::Bash::new);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "nonexistent",
                "arguments": {}
            }),
        };
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_method_not_found() {
        let mut server = McpServer::new(bashkit::Bash::new);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "unknown/method".to_string(),
            params: serde_json::json!({}),
        };
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.expect("error").code, -32601);
    }

    #[tokio::test]
    async fn test_parse_error_sanitized() {
        // Malformed JSON should return generic "Parse error", not internal details
        let malformed = "not valid json {{{";
        let err = serde_json::from_str::<JsonRpcRequest>(malformed).unwrap_err();
        let response =
            JsonRpcResponse::error(serde_json::Value::Null, -32700, "Parse error".to_string());
        let serialized = serde_json::to_string(&response).unwrap();
        // Must not contain serde internal details like "line", "column", struct names
        assert!(!serialized.contains("line"));
        assert!(!serialized.contains("column"));
        assert!(!serialized.contains("expected"));
        // But the raw error DOES contain those details (confirming we'd leak them without sanitization)
        let raw = format!("{}", err);
        assert!(raw.contains("line") || raw.contains("column") || raw.contains("expected"));
        // Response message is exactly "Parse error"
        let resp_obj: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(resp_obj["error"]["message"], "Parse error");
    }

    #[tokio::test]
    async fn test_invalid_params_sanitized() {
        let mut server = McpServer::new(bashkit::Bash::new);
        // Call bash tool with wrong argument shape (missing "script")
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "wrong_field": 123 }
            }),
        };
        let resp = server.handle_request(req).await;
        let err = resp.error.expect("should have error");
        assert_eq!(err.code, -32602);
        assert_eq!(err.message, "Invalid parameters");
        // Must not contain Rust type names or serde details
        assert!(!err.message.contains("missing field"));
        assert!(!err.message.contains("BashToolArgs"));
    }

    #[tokio::test]
    async fn test_execution_error_sanitized() {
        // Use a factory that returns a Bash with max_commands=0 to force immediate error
        let mut server = McpServer::new(|| {
            bashkit::Bash::builder()
                .limits(bashkit::ExecutionLimits::new().max_commands(0))
                .build()
        });
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "script": "echo test" }
            }),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        let text = result["content"][0]["text"].as_str().expect("text");
        // If it's an error result, it should say "Execution failed", not internal details
        if result["isError"] == true {
            assert_eq!(text, "Execution failed");
        }
    }

    #[tokio::test]
    async fn test_tools_call_respects_max_commands() {
        // Factory that creates a Bash with max_commands=2
        let mut server = McpServer::new(|| {
            bashkit::Bash::builder()
                .limits(bashkit::ExecutionLimits::new().max_commands(2))
                .build()
        });

        // Script with 3 commands should hit the limit
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "script": "echo a; echo b; echo c" }
            }),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        let text = result["content"][0]["text"].as_str().expect("text");
        // Should report the limit was exceeded
        assert!(
            text.contains("limit") || text.contains("exceeded") || result["isError"] == true,
            "expected execution limit error, got: {text}"
        );
    }

    #[tokio::test]
    async fn test_session_limits_accumulate_across_mcp_calls() {
        // Session limit: max 3 total commands across all calls, max 2 exec calls.
        let mut server = McpServer::new(|| {
            bashkit::Bash::builder()
                .session_limits(
                    bashkit::SessionLimits::new()
                        .max_total_commands(3)
                        .max_exec_calls(2),
                )
                .build()
        });

        // First call: 2 commands. Should succeed.
        let req1 = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "script": "echo a; echo b" }
            }),
        };
        let resp1 = server.handle_request(req1).await;
        let result1 = resp1.result.expect("should have result");
        let text1 = result1["content"][0]["text"].as_str().expect("text");
        assert!(
            text1.contains('a') && text1.contains('b'),
            "first call should succeed, got: {text1}"
        );

        // Second call: 2 more commands -> cumulative 4 > limit of 3. Should fail.
        let req2 = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(2),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "script": "echo c; echo d" }
            }),
        };
        let resp2 = server.handle_request(req2).await;
        let result2 = resp2.result.expect("should have result");
        let text2 = result2["content"][0]["text"].as_str().expect("text");
        assert!(
            text2.contains("session") || text2.contains("limit") || result2["isError"] == true,
            "second call should hit session limit, got: {text2}"
        );
    }

    #[tokio::test]
    async fn test_session_exec_calls_accumulate_across_mcp_calls() {
        // Session limit: max 2 exec calls.
        let mut server = McpServer::new(|| {
            bashkit::Bash::builder()
                .session_limits(bashkit::SessionLimits::new().max_exec_calls(2))
                .build()
        });

        // First two calls should succeed.
        for i in 1..=2 {
            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(i),
                method: "tools/call".to_string(),
                params: serde_json::json!({
                    "name": "bash",
                    "arguments": { "script": "echo ok" }
                }),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            let text = result["content"][0]["text"].as_str().expect("text");
            assert!(text.contains("ok"), "call {i} should succeed, got: {text}");
        }

        // Third call should hit session exec call limit.
        let req3 = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(3),
            method: "tools/call".to_string(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": { "script": "echo should_fail" }
            }),
        };
        let resp3 = server.handle_request(req3).await;
        let result3 = resp3.result.expect("should have result");
        let text3 = result3["content"][0]["text"].as_str().expect("text");
        assert!(
            text3.contains("session") || text3.contains("limit") || result3["isError"] == true,
            "third call should hit session exec call limit, got: {text3}"
        );
    }

    #[cfg(feature = "scripted_tool")]
    mod scripted_tool_tests {
        use super::*;
        use bashkit::{ScriptedTool, ToolArgs, ToolDef};

        fn make_test_tool() -> ScriptedTool {
            ScriptedTool::builder("test_api")
                .short_description("Test API tool")
                .tool(ToolDef::new("greet", "Greet someone"), |args: &ToolArgs| {
                    let name = args.param_str("name").unwrap_or("world");
                    Ok(format!("hello {name}\n"))
                })
                .build()
        }

        #[tokio::test]
        async fn test_tools_list_includes_scripted_tool() {
            let mut server = McpServer::new(bashkit::Bash::new);
            server.register_scripted_tool(make_test_tool());

            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(1),
                method: "tools/list".to_string(),
                params: serde_json::json!({}),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            let tools = result["tools"].as_array().expect("tools array");
            assert!(tools.iter().any(|t| t["name"] == "bash"));
            assert!(tools.iter().any(|t| t["name"] == "test_api"));
        }

        #[tokio::test]
        async fn test_tools_call_scripted_tool() {
            let mut server = McpServer::new(bashkit::Bash::new);
            server.register_scripted_tool(make_test_tool());

            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(1),
                method: "tools/call".to_string(),
                params: serde_json::json!({
                    "name": "test_api",
                    "arguments": { "commands": "greet --name Alice" }
                }),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            let text = result["content"][0]["text"].as_str().expect("text");
            assert!(text.contains("hello Alice"));
        }

        #[tokio::test]
        async fn test_tools_call_scripted_tool_error() {
            let mut server = McpServer::new(bashkit::Bash::new);
            let tool = ScriptedTool::builder("err_api")
                .short_description("Error API")
                .tool(ToolDef::new("fail", "Always fails"), |_args: &ToolArgs| {
                    Err("service down".to_string())
                })
                .build();
            server.register_scripted_tool(tool);

            let req = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(1),
                method: "tools/call".to_string(),
                params: serde_json::json!({
                    "name": "err_api",
                    "arguments": { "commands": "fail" }
                }),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            assert_eq!(result["isError"], true);
        }

        #[tokio::test]
        async fn test_full_jsonrpc_roundtrip() {
            let mut server = McpServer::new(bashkit::Bash::new);
            server.register_scripted_tool(make_test_tool());

            // Step 1: initialize
            let init_resp = server
                .handle_request(JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    id: serde_json::json!(1),
                    method: "initialize".to_string(),
                    params: serde_json::json!({}),
                })
                .await;
            assert!(init_resp.result.is_some());

            // Step 2: tools/list
            let list_resp = server
                .handle_request(JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    id: serde_json::json!(2),
                    method: "tools/list".to_string(),
                    params: serde_json::json!({}),
                })
                .await;
            let list_result = list_resp.result.expect("result");
            let tools = list_result["tools"].as_array().expect("tools");
            assert!(tools.len() >= 2);

            // Step 3: tools/call
            let call_resp = server
                .handle_request(JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    id: serde_json::json!(3),
                    method: "tools/call".to_string(),
                    params: serde_json::json!({
                        "name": "test_api",
                        "arguments": { "commands": "greet --name MCP" }
                    }),
                })
                .await;
            let call_result = call_resp.result.expect("result");
            let text = call_result["content"][0]["text"].as_str().expect("text");
            assert!(text.contains("hello MCP"));
        }
    }
}
