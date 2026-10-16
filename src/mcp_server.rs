use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use crate::buffer_manager::{BufferManager, BufferCommand};
use crate::keyboard_hook::KeyboardHook;
use crate::keyboard_reader::spawn_key_event_reader;

#[cfg(windows)]
use winapi::um::{consoleapi, wincon};

#[cfg(windows)]
fn ensure_console() -> Result<()> {
    // Try to attach to parent console or allocate a new one
    unsafe {
        // First try to attach to parent process console
        // ATTACH_PARENT_PROCESS = -1 (as DWORD)
        if winapi::um::wincon::AttachConsole(-1i32 as u32) != 0 {
            return Ok(());
        }
        // If that fails, allocate a new console
        if winapi::um::consoleapi::AllocConsole() != 0 {
            // Redirect stdin/stdout/stderr to the new console
            let _ = std::io::stdin();
            let _ = std::io::stdout();
            let _ = std::io::stderr();
            return Ok(());
        }
    }
    Err(anyhow::anyhow!("Failed to allocate console for TUI"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResult {
    protocol_version: String,
    capabilities: ServerCapabilities,
    server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerCapabilities {
    tools: ToolsCapability,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsCapability {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsListResult {
    tools: Vec<Tool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: String,
    description: String,
    input_schema: ToolInputSchema,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolInputSchema {
    #[serde(rename = "type")]
    schema_type: String,
    properties: Value,
    required: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallResult {
    content: Vec<ToolContent>,
    is_error: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallParams {
    name: String,
    arguments: Value,
}

// Global state for lazy TUI initialization
static TUI_SPAWNED: OnceLock<()> = OnceLock::new();

/// Spawns TUI and keyboard reader in a background thread.
/// Only runs once - subsequent calls are no-ops.
/// Wrapped in catch_unwind to prevent panics from killing the MCP server.
fn spawn_tui_lazily(buffer_manager: BufferManager) {
    TUI_SPAWNED.get_or_init(|| {
        tracing::info!("Lazy-spawning TUI on first tools/call");

        // Spawn in a separate thread with panic catching
        std::thread::spawn(move || {
            // Catch any panics from Crossterm/console initialization
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                // On Windows, ensure we have a console for TUI
                #[cfg(windows)]
                if let Err(e) = ensure_console() {
                    tracing::warn!("Could not allocate console for TUI: {}", e);
                    return;
                }

                tracing::info!("Initializing KeyboardHook for TUI");
                let keyboard_hook = KeyboardHook::new(buffer_manager.clone());

                // Spawn keyboard event reader (reads from /dev/tty on Unix)
                let key_event_tx = keyboard_hook.get_key_event_sender();
                spawn_key_event_reader(key_event_tx, true);
                tracing::info!("Keyboard event reader spawned for MCP mode");

                // Run TUI loop
                if let Err(e) = keyboard_hook.start_tui() {
                    tracing::error!("TUI loop error: {}", e);
                }
            }));

            if let Err(panic_payload) = result {
                tracing::error!("TUI thread panicked: {:?}", panic_payload);
            }
        });
    });
}

pub async fn run_mcp_server(buffer_manager: BufferManager) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        
        if bytes_read == 0 {
            // EOF
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(req) => req,
            Err(e) => {
                let response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                write_response(&mut stdout, &response).await?;
                continue;
            }
        };

        let response = handle_request(request, &buffer_manager).await;
        write_response(&mut stdout, &response).await?;
    }

    Ok(())
}

async fn write_response(stdout: &mut tokio::io::Stdout, response: &JsonRpcResponse) -> Result<()> {
    let json = serde_json::to_string(response)?;
    stdout.write_all(json.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}

async fn handle_request(request: JsonRpcRequest, buffer_manager: &BufferManager) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request: jsonrpc must be \"2.0\"".to_string(),
                data: None,
            }),
        };
    }

    match request.method.as_str() {
        "initialize" => handle_initialize(request.id),
        "tools/list" => handle_tools_list(request.id),
        "tools/call" => handle_tools_call(request.id, request.params, buffer_manager).await,
        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        },
    }
}

fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
    let result = InitializeResult {
        protocol_version: "2024-11-05".to_string(),
        capabilities: ServerCapabilities {
            tools: ToolsCapability {},
        },
        server_info: ServerInfo {
            name: "sloptyper".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    };

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    }
}

fn handle_tools_list(id: Option<Value>) -> JsonRpcResponse {
    let result = ToolsListResult {
        tools: vec![Tool {
            name: "stream_code".to_string(),
            description: "Stream generated code directly into the SlopTyper typing buffer.".to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: json!({
                    "code": {
                        "type": "string",
                        "description": "The code to stream into the typing buffer"
                    }
                }),
                required: vec!["code".to_string()],
            },
        }],
    };

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    }
}

async fn handle_tools_call(
    id: Option<Value>,
    params: Option<Value>,
    buffer_manager: &BufferManager,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: "Invalid params: missing parameters".to_string(),
                    data: None,
                }),
            };
        }
    };

    let tool_params: ToolCallParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", e),
                    data: None,
                }),
            };
        }
    };

    if tool_params.name != "stream_code" {
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Unknown tool: {}", tool_params.name),
                data: None,
            }),
        };
    }

    // Lazy-spawn TUI on first stream_code call
    spawn_tui_lazily(buffer_manager.clone());

    let code = match tool_params.arguments.get("code").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: "Invalid arguments: missing required 'code' property".to_string(),
                    data: None,
                }),
            };
        }
    };

    // Clean the incoming text (extract code blocks if present)
    let cleaned = crate::llm_stream::clean_incoming_text(&code);
    
    if cleaned.is_empty() {
        let result = ToolCallResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text: "No code extracted from input".to_string(),
            }],
            is_error: true,
        };
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        };
    }

    // Load the code into the buffer
    let _ = buffer_manager.get_command_sender().send(BufferCommand::Clear);
    buffer_manager.process_commands();
    
    let _ = buffer_manager.get_command_sender().send(BufferCommand::Load(cleaned.clone()));
    buffer_manager.process_commands();

    let result = ToolCallResult {
        content: vec![ToolContent {
            content_type: "text".to_string(),
            text: format!("Loaded {} characters into buffer", cleaned.len()),
        }],
        is_error: false,
    };

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    }
}