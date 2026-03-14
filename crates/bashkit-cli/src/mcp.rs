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
pub struct McpServer {
    #[cfg(feature = "scripted_tool")]
    scripted_tools: Vec<bashkit::ScriptedTool>,
}

impl McpServer {
    /// Create a new MCP server with only the default `bash` tool.
    pub fn new() -> Self {
        Self {
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

        JsonRpcResponse::success(id, serde_json::to_value(result).expect("serialize init"))
    }

    fn handle_tools_list(&self, id: serde_json::Value) -> JsonRpcResponse {
        let tools = vec![McpTool {
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
                return JsonRpcResponse::error(id, -32602, format!("Invalid arguments: {}", e));
            }
        };

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
                    serde_json::to_value(tool_result).expect("serialize"),
                );
            }
        };

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

/// Run the MCP server (backward-compatible entry point).
pub async fn run() -> Result<()> {
    let mut server = McpServer::new();
    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_initialize() {
        let mut server = McpServer::new();
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
        let mut server = McpServer::new();
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
        let mut server = McpServer::new();
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
        let mut server = McpServer::new();
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
        let mut server = McpServer::new();
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
            let mut server = McpServer::new();
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
            let mut server = McpServer::new();
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
            let mut server = McpServer::new();
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
            let mut server = McpServer::new();
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
