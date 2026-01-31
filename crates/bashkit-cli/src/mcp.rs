//! MCP (Model Context Protocol) server implementation
//!
//! Implements a JSON-RPC 2.0 server that exposes bashkit as an MCP tool.
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
struct Tool {
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

/// Run the MCP server
pub async fn run() -> Result<()> {
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

        let response = handle_request(request).await;
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}

async fn handle_request(request: JsonRpcRequest) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request.id),
        "initialized" => JsonRpcResponse::success(request.id, serde_json::Value::Null),
        "tools/list" => handle_tools_list(request.id),
        "tools/call" => handle_tools_call(request.id, request.params).await,
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

    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
}

fn handle_tools_list(id: serde_json::Value) -> JsonRpcResponse {
    let tools = vec![Tool {
        name: "bash".to_string(),
        description: "Execute a bash script in a sandboxed environment".to_string(),
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

    JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
}

async fn handle_tools_call(id: serde_json::Value, params: serde_json::Value) -> JsonRpcResponse {
    // Extract tool name and arguments
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or_default();

    if tool_name != "bash" {
        return JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name));
    }

    // Parse arguments
    let args: BashToolArgs = match serde_json::from_value(arguments) {
        Ok(a) => a,
        Err(e) => {
            return JsonRpcResponse::error(id, -32602, format!("Invalid arguments: {}", e));
        }
    };

    // Execute the script
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
            return JsonRpcResponse::success(id, serde_json::to_value(tool_result).unwrap());
        }
    };

    // Format output
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

    JsonRpcResponse::success(id, serde_json::to_value(tool_result).unwrap())
}
