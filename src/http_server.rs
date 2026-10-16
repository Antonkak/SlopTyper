use axum::{
    routing::post,
    Router,
    extract::State,
    response::IntoResponse,
};
use serde::Deserialize;
use crate::buffer_manager::{BufferManager, BufferCommand};
use crate::llm_stream::clean_incoming_text;

#[derive(Debug, Deserialize)]
struct StreamRequest {
    text: String,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub buffer_manager: BufferManager,
}

async fn stream_handler(
    State(state): State<AppState>,
    body: String,
) -> impl IntoResponse {
    tracing::info!("Received stream request: {} bytes", body.len());
    
    // Try to parse as JSON first
    let text = if let Ok(req) = serde_json::from_str::<StreamRequest>(&body) {
        req.text
    } else {
        // Use raw text directly
        body
    };
    
    // Clean the text by extracting code blocks
    let cleaned = clean_incoming_text(&text);
    
    if cleaned.is_empty() {
        tracing::warn!("No code extracted from input");
        return "No code extracted".to_string();
    }
    
    tracing::info!("Cleaned code ({} chars): {}", cleaned.len(), cleaned);
    
    // Clear the buffer and load new content
    let _ = state.buffer_manager.get_command_sender().send(BufferCommand::Clear);
    state.buffer_manager.process_commands();
    
    let _ = state.buffer_manager.get_command_sender().send(BufferCommand::Append(cleaned.clone()));
    state.buffer_manager.process_commands();
    
    tracing::info!("Loaded {} characters into buffer", cleaned.len());
    
    format!("OK, loaded {} characters into buffer", cleaned.len())
}

pub async fn start_server(addr: &str, state: AppState) -> Result<(), anyhow::Error> {
    let app = Router::new()
        .route("/stream", post(stream_handler))
        .with_state(state);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP server listening on {}", addr);
    
    axum::serve(listener, app).await?;
    Ok(())
}
