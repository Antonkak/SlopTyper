use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use rand::Rng;
use crate::buffer_manager::{BufferManager, BufferCommand, generate_sample_code};

#[derive(Debug, Clone)]
pub struct Token {
    pub text: String,
    pub is_final: bool,
}

pub struct LlmStream {
    buffer_manager: BufferManager,
    token_tx: mpsc::Sender<Token>,
    token_rx: mpsc::Receiver<Token>,
    is_streaming: bool,
}

impl LlmStream {
    pub fn new(buffer_manager: BufferManager) -> Self {
        let (token_tx, token_rx) = mpsc::channel(100);
        
        Self {
            buffer_manager,
            token_tx,
            token_rx,
            is_streaming: false,
        }
    }

    pub fn get_token_sender(&self) -> mpsc::Sender<Token> {
        self.token_tx.clone()
    }

    pub fn get_token_receiver(&mut self) -> mpsc::Receiver<Token> {
        std::mem::replace(&mut self.token_rx, mpsc::channel(100).1)
    }

    pub async fn start_mock_stream(&mut self) -> anyhow::Result<()> {
        if self.is_streaming {
            return Ok(());
        }
        self.is_streaming = true;

        let code = generate_sample_code();
        tracing::info!("Generated sample code (length: {}), loading into buffer...", code.len());
        self.buffer_manager.get_command_sender().send(BufferCommand::Load(code.clone()))?;

        let sender = self.token_tx.clone();
        tokio::spawn(async move {
            let chars: Vec<char> = code.chars().collect();
            tracing::info!("Starting to send {} tokens...", chars.len());
            let mut idx = 0;
            
            while idx < chars.len() {
                if sender.send(Token { text: chars[idx].to_string(), is_final: false }).await.is_err() {
                    break;
                }
                let delay = rand::thread_rng().gen_range(10..=50);
                sleep(Duration::from_millis(delay)).await;
                idx += 1;
            }
            
            let _ = sender.send(Token { text: String::new(), is_final: true }).await;
            tracing::info!("All tokens sent, stream complete");
        });

        Ok(())
    }

    pub fn stop(&mut self) {
        self.is_streaming = false;
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }
}

pub async fn run_token_consumer(
    mut token_rx: mpsc::Receiver<Token>,
    buffer_manager: BufferManager,
) {
    tracing::info!("Token consumer started, waiting for tokens...");
    let mut count = 0;
    let mut accumulated = String::new();
    
    while let Some(token) = token_rx.recv().await {
        if token.is_final {
            tracing::info!("LLM stream completed, total tokens received: {}", count);
            
            // Final extraction of any remaining code blocks
            let cleaned = if accumulated.contains("```") {
                let text = accumulated.trim();
                if text.contains("```") {
                    let mut result = String::new();
                    let mut in_code_block = false;
                    let mut code_block_content = String::new();
                    for line in text.lines() {
                        if line.trim_start().starts_with("```") {
                            if in_code_block {
                                result.push_str(&code_block_content.trim_end());
                                result.push('\n');
                                code_block_content.clear();
                                in_code_block = false;
                            } else {
                                in_code_block = true;
                            }
                        } else if in_code_block {
                            code_block_content.push_str(line);
                            code_block_content.push('\n');
                        }
                    }
                    if in_code_block && !code_block_content.is_empty() {
                        result.push_str(&code_block_content.trim_end());
                        result.push('\n');
                    }
                    result
                } else {
                    text.to_string()
                }
            } else {
                accumulated.trim().to_string()
            };
            
            if !cleaned.is_empty() {
                let _ = buffer_manager.get_command_sender().send(BufferCommand::Append(cleaned));
            }
            buffer_manager.process_commands();
            break;
        }
        count += 1;
        tracing::info!("Received token {}: '{}'", count, token.text);
        
        accumulated.push_str(&token.text);
        
        // Try to extract complete code blocks from accumulated text
        let cleaned = extract_code_blocks(&accumulated);
        if !cleaned.is_empty() {
            // Clear the processed part from accumulated
            accumulated.clear();
            let _ = buffer_manager.get_command_sender().send(BufferCommand::Append(cleaned));
        }
        buffer_manager.process_commands();
    }
}

/// Extract code blocks from markdown text.
/// If triple backticks are found, extract content from complete code blocks.
/// If no code blocks, return the text as-is.
/// Returns empty string if there's an incomplete code block (opening ``` without closing).
pub fn extract_code_blocks(text: &str) -> String {
    let trimmed = text.trim();
    
    // Check if the text contains triple backticks
    if !trimmed.contains("```") {
        return text.to_string();
    }
    
    // Extract all complete code blocks
    let mut result = String::new();
    let mut in_code_block = false;
    let mut code_block_content = String::new();
    let mut has_incomplete_block = false;
    
    for line in trimmed.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                // End of code block
                result.push_str(&code_block_content.trim_end());
                result.push('\n');
                code_block_content.clear();
                in_code_block = false;
                has_incomplete_block = false;
            } else {
                // Start of code block - skip this line (may contain language specifier)
                in_code_block = true;
                has_incomplete_block = true;
            }
        } else if in_code_block {
            code_block_content.push_str(line);
            code_block_content.push('\n');
        }
    }
    
    // If we're still in a code block, we have an incomplete block
    if has_incomplete_block {
        // Don't return partial content - the caller should keep buffering
        return String::new();
    }
    
    // Handle case where code block doesn't have closing backticks (but was opened)
    if in_code_block && !code_block_content.is_empty() {
        // Don't return incomplete block content
        return String::new();
    }
    
    if result.is_empty() {
        return text.to_string();
    }
    
    result
}

/// Clean incoming text by extracting code blocks if present
pub fn clean_incoming_text(text: &str) -> String {
    extract_code_blocks(text)
}