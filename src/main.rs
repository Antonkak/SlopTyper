mod buffer_manager;
mod llm_stream;
mod keyboard_hook;
mod config;
mod stdin_handler;
mod keyboard_reader;
mod mcp_server;

use anyhow::Result;
use clap::Parser;
use tracing::{info, error};
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt};

use buffer_manager::BufferManager;
use keyboard_hook::KeyboardHook;
use config::Config;
use stdin_handler::is_stdin_piped;
use keyboard_reader::spawn_key_event_reader;
use mcp_server::run_mcp_server;

#[tokio::main]
async fn main() -> Result<()> {
    // Set up panic hook to log panics to file (critical for MCP mode where panics in TUI thread shouldn't kill JSON-RPC)
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let msg = format!("PANIC: {:?}", panic_info);
        tracing::error!("{}", msg);
        // Also print to stderr for visibility
        eprintln!("{}", msg);
        orig_hook(panic_info);
    }));

    let config = Config::parse();

    // Setup logging - in MCP mode, logs must go to stderr or file only
    if config.mcp {
        // MCP mode: log to file only, stdout must be clean for JSON-RPC
        let file_appender = rolling::never(".", "sloptyper.log");
        fmt()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("sloptyper=info"))
            )
            .init();
    } else {
        // Normal mode: log to file
        let file_appender = rolling::never(".", "sloptyper.log");
        fmt()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_env_filter(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("sloptyper=info"))
            )
            .init();
    }

    info!("--- SLOPTYPER TUI STARTED ---");
    info!("Model: {}", config.model);
    if config.get_api_key().is_some() {
        info!("API key: configured");
    }
    if let Some(ref base_url) = config.base_url {
        info!("Base URL: {}", base_url);
    }
    info!("Press any key in the TUI to inject code, Esc/Enter to copy and exit");

    let buffer_manager = BufferManager::new(String::new(), (5, 8));

    if config.mcp {
        // MCP mode: run JSON-RPC server on stdin/stdout
        // TUI and keyboard will be lazy-spawned on first tools/call
        info!("Starting MCP server on STDIN/STDOUT (TUI will start lazily on first stream_code)");

        // Run MCP server (this blocks on stdin/stdout)
        if let Err(e) = run_mcp_server(buffer_manager).await {
            error!("MCP server error: {}", e);
        }
    } else {
        // Normal mode: check stdin pipe, HTTP server, sample code
        let stdin_piped = is_stdin_piped();
        info!("STDIN piped: {}", stdin_piped);

        let keyboard_hook = KeyboardHook::new(buffer_manager.clone());

        if stdin_piped {
            let buffer_manager_for_stdin = buffer_manager.clone();
            tokio::spawn(async move {
                if let Err(e) = stdin_handler::spawn_stdin_reader(buffer_manager_for_stdin).await {
                    tracing::error!("STDIN reader error: {}", e);
                }
            });
            info!("STDIN reader spawned, waiting for piped input...");
        } else {
            let sample_code = buffer_manager::generate_sample_code();
            let _ = buffer_manager.get_command_sender().send(
                buffer_manager::BufferCommand::Load(sample_code.clone())
            );
            buffer_manager.process_commands();
            
            info!("Loaded {} characters into buffer", sample_code.len());
        }

        // Start HTTP server for AI agents
        let buffer_manager_for_http = buffer_manager.clone();
        
        tokio::spawn(async move {
            use axum::{
                routing::post,
                Router,
                extract::State,
                response::IntoResponse,
            };
            use serde::Deserialize;

            #[derive(Debug, Deserialize)]
            struct StreamRequest {
                text: String,
            }

            #[derive(Debug, Clone)]
            struct AppState {
                buffer_manager: BufferManager,
            }

            async fn stream_handler(
                State(state): State<AppState>,
                body: String,
            ) -> impl IntoResponse {
                let text = if let Ok(req) = serde_json::from_str::<StreamRequest>(&body) {
                    req.text
                } else {
                    body
                };
                
                let cleaned = llm_stream::clean_incoming_text(&text);
                
                if cleaned.is_empty() {
                    return "No code extracted".to_string();
                }
                
                tracing::info!("Loaded {} characters into buffer", cleaned.len());
                
                let _ = state.buffer_manager.get_command_sender().send(buffer_manager::BufferCommand::Clear);
                state.buffer_manager.process_commands();
                
                let _ = state.buffer_manager.get_command_sender().send(buffer_manager::BufferCommand::Append(cleaned.clone()));
                state.buffer_manager.process_commands();
                
                format!("OK, loaded {} characters into buffer", cleaned.len())
            }

            let app_state = AppState {
                buffer_manager: buffer_manager_for_http,
            };

            let app = Router::new()
                .route("/stream", post(stream_handler))
                .with_state(app_state);

            let listener = tokio::net::TcpListener::bind("127.0.0.1:8765").await.unwrap();
            tracing::info!("HTTP server listening on 127.0.0.1:8765");
            
            axum::serve(listener, app).await.unwrap();
        });

        // Spawn keyboard event reader thread
        let key_event_tx = keyboard_hook.get_key_event_sender();
        spawn_key_event_reader(key_event_tx, stdin_piped);
        info!("Keyboard event reader spawned (stdin_piped: {})", stdin_piped);

        // Run TUI event loop (starts in active mode)
        if let Err(e) = keyboard_hook.start_tui() {
            error!("TUI loop error: {}", e);
        }
    }

    info!("SlopTyper exited");

    Ok(())
}