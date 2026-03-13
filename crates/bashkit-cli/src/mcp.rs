//! MCP (Model Context Protocol) server implementation
//!
//! Implements a JSON-RPC 2.0 server that exposes bashkit as an MCP tool.
//! Supports registering ScriptedTool instances as additional MCP tools.
//!
//! Protocol:
//! - Input: JSON-RPC requests on stdin
//! - Output: JSON-RPC responses on stdout

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

#[cfg(feature = "scripted_tool")]
use bashkit::tool::{Tool, ToolRequest};

/// JSON-RPC 2.0 request
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)] // Required by JSON-RPC spec but not used in routing
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
struct McpToolDef {
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

/// MCP server with optional ScriptedTool registration.
pub struct McpServer {
    #[cfg(feature = "scripted_tool")]
    scripted_tools: Vec<bashkit::ScriptedTool>,
}

impl McpServer {
    /// Create a new MCP server.
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "scripted_tool")]
            scripted_tools: Vec::new(),
        }
    }

    /// Register a ScriptedTool to be exposed as an MCP tool.
    ///
    /// Each registered ScriptedTool appears as a single MCP tool in `tools/list`.
    /// The tool accepts `{script: "<bash>"}` and routes to `ScriptedTool::execute()`.
    #[cfg(feature = "scripted_tool")]
    #[allow(dead_code)] // Public API for programmatic use; not called by CLI binary
    pub fn register_scripted_tool(&mut self, tool: bashkit::ScriptedTool) {
        self.scripted_tools.push(tool);
    }

    /// Run the MCP server, reading JSON-RPC from stdin and writing responses to stdout.
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
                    let response = JsonRpcResponse::error(
                        serde_json::Value::Null,
                        -32700,
                        format!("Parse error: {}", e),
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

        JsonRpcResponse::success(
            id,
            serde_json::to_value(result).expect("serialize init result"),
        )
    }

    fn handle_tools_list(&self, id: serde_json::Value) -> JsonRpcResponse {
        let mut tools = vec![McpToolDef {
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
        for st in &self.scripted_tools {
            tools.push(McpToolDef {
                name: st.name().to_string(),
                description: st.description(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "script": {
                            "type": "string",
                            "description": format!("Bash script using {} tool commands", st.name())
                        }
                    },
                    "required": ["script"]
                }),
            });
        }

        JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
    }

    async fn handle_tools_call(
        &mut self,
        id: serde_json::Value,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let arguments = params.get("arguments").cloned().unwrap_or_default();

        // Parse script argument
        let args: BashToolArgs = match serde_json::from_value(arguments) {
            Ok(a) => a,
            Err(e) => {
                return JsonRpcResponse::error(id, -32602, format!("Invalid arguments: {}", e));
            }
        };

        // Route to ScriptedTool if registered
        #[cfg(feature = "scripted_tool")]
        {
            if let Some(st) = self
                .scripted_tools
                .iter_mut()
                .find(|t| t.name() == tool_name)
            {
                let resp = st
                    .execute(ToolRequest {
                        commands: args.script,
                        timeout_ms: None,
                    })
                    .await;
                return Self::format_tool_response(id, &resp.stdout, &resp.stderr, resp.exit_code);
            }
        }

        if tool_name != "bash" {
            return JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name));
        }

        // Execute via plain bash
        let mut bash = bashkit::Bash::new();
        let result = match bash.exec(&args.script).await {
            Ok(r) => r,
            Err(e) => {
                let tool_result = ToolResult {
                    content: vec![ContentItem {
                        content_type: "text".to_string(),
                        text: format!("Error: {}", e),
                    }],
                    is_error: Some(true),
                };
                return JsonRpcResponse::success(
                    id,
                    serde_json::to_value(tool_result).expect("serialize error result"),
                );
            }
        };

        Self::format_tool_response(id, &result.stdout, &result.stderr, result.exit_code)
    }

    fn format_tool_response(
        id: serde_json::Value,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> JsonRpcResponse {
        let mut output = stdout.to_string();
        if !stderr.is_empty() {
            output.push_str("\n[stderr]\n");
            output.push_str(stderr);
        }
        if exit_code != 0 {
            output.push_str(&format!("\n[exit code: {}]", exit_code));
        }

        let tool_result = ToolResult {
            content: vec![ContentItem {
                content_type: "text".to_string(),
                text: output,
            }],
            is_error: if exit_code != 0 { Some(true) } else { None },
        };

        JsonRpcResponse::success(
            id,
            serde_json::to_value(tool_result).expect("serialize tool result"),
        )
    }
}

/// Run the MCP server (convenience function for backward compatibility).
pub async fn run() -> Result<()> {
    McpServer::new().run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_initialize() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "initialize".into(),
            params: serde_json::json!({}),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "bashkit");
    }

    #[tokio::test]
    async fn test_tools_list_has_bash() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "tools/list".into(),
            params: serde_json::json!({}),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        let tools = result["tools"].as_array().expect("tools array");
        assert!(tools.iter().any(|t| t["name"] == "bash"));
    }

    #[tokio::test]
    async fn test_tools_call_bash() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "tools/call".into(),
            params: serde_json::json!({
                "name": "bash",
                "arguments": {"script": "echo hello"}
            }),
        };
        let resp = server.handle_request(req).await;
        let result = resp.result.expect("should have result");
        let text = &result["content"][0]["text"];
        assert!(text.as_str().expect("text").contains("hello"));
    }

    #[tokio::test]
    async fn test_unknown_tool_error() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "tools/call".into(),
            params: serde_json::json!({
                "name": "nonexistent",
                "arguments": {"script": "echo hi"}
            }),
        };
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some());
        assert!(resp.error.expect("error").message.contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: serde_json::json!(1),
            method: "unknown/method".into(),
            params: serde_json::json!({}),
        };
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some());
    }

    #[cfg(feature = "scripted_tool")]
    mod scripted_tool_tests {
        use super::*;
        use bashkit::{ScriptedTool, ToolArgs, ToolDef};

        fn make_test_tool() -> ScriptedTool {
            ScriptedTool::builder("greeter")
                .short_description("Greeting API")
                .tool(
                    ToolDef::new("greet", "Greet a user").with_schema(serde_json::json!({
                        "type": "object",
                        "properties": { "name": {"type": "string"} }
                    })),
                    |args: &ToolArgs| {
                        let name = args.param_str("name").unwrap_or("world");
                        Ok(format!("hello {name}\n"))
                    },
                )
                .build()
        }

        #[tokio::test]
        async fn test_register_scripted_tool_in_list() {
            let mut server = McpServer::new();
            server.register_scripted_tool(make_test_tool());

            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "tools/list".into(),
                params: serde_json::json!({}),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            let tools = result["tools"].as_array().expect("tools array");
            assert!(tools.iter().any(|t| t["name"] == "bash"));
            assert!(tools.iter().any(|t| t["name"] == "greeter"));
        }

        #[tokio::test]
        async fn test_scripted_tool_call() {
            let mut server = McpServer::new();
            server.register_scripted_tool(make_test_tool());

            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "tools/call".into(),
                params: serde_json::json!({
                    "name": "greeter",
                    "arguments": {"script": "greet --name Alice"}
                }),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            let text = result["content"][0]["text"].as_str().expect("text string");
            assert!(text.contains("hello Alice"));
        }

        #[tokio::test]
        async fn test_scripted_tool_error_handling() {
            let mut server = McpServer::new();
            let tool = ScriptedTool::builder("errtest")
                .tool(ToolDef::new("fail", "Always fails"), |_: &ToolArgs| {
                    Err("broken".to_string())
                })
                .build();
            server.register_scripted_tool(tool);

            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "tools/call".into(),
                params: serde_json::json!({
                    "name": "errtest",
                    "arguments": {"script": "fail"}
                }),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            assert_eq!(result["isError"], true);
        }

        #[tokio::test]
        async fn test_bash_tool_still_works_with_scripted() {
            let mut server = McpServer::new();
            server.register_scripted_tool(make_test_tool());

            let req = JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "tools/call".into(),
                params: serde_json::json!({
                    "name": "bash",
                    "arguments": {"script": "echo plain bash"}
                }),
            };
            let resp = server.handle_request(req).await;
            let result = resp.result.expect("should have result");
            let text = result["content"][0]["text"].as_str().expect("text string");
            assert!(text.contains("plain bash"));
        }
    }
}
