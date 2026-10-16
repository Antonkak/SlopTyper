use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "SlopTyper")]
#[command(about = "TUI Hacker Typer - Type code like a hacker")]
pub struct Config {
    /// Model name (default: deepseek-chat)
    #[arg(long, default_value = "deepseek-chat")]
    pub model: String,

    /// API key for the LLM service (or use OPENAI_API_KEY env var)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Base URL for the API (optional, for Ollama/LocalLLM)
    #[arg(long)]
    pub base_url: Option<String>,

    /// Run as MCP server over STDIN/STDOUT
    #[arg(long)]
    pub mcp: bool,
}

impl Config {
    pub fn get_api_key(&self) -> Option<String> {
        self.api_key.clone().or_else(|| {
            std::env::var("OPENAI_API_KEY").ok()
        })
    }
}
