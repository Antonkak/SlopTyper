# SlopTyper - TUI Hacker Typer

<p align="center">
  <img src="https://raw.githubusercontent.com/Antonkak/SlopTyper/media/gifs/sloptyper_intro_1.gif" width="600" alt="SlopTyper Demo">
</p>

A terminal-based "hacker typer" that injects AI-generated code into your terminal when you type. Runs as a TUI (Terminal User Interface) application using crossterm.
> [!NOTE]
> ### Fake your productivity in blazingly fast Rust
> _"I wanted to pretend I was writing in Rust, so I made SlopTyper in Rust."_
## Features

- **TUI Interface**: Full-screen terminal UI with raw mode input handling
- **HTTP Server**: Runs a local HTTP server on `127.0.0.1:8765` to receive code from AI agents
- **Keystroke Injection**: Every key press in the TUI injects code from the buffer
- **Batch Injection**: Injects 5-8 characters per keystroke for realistic "hacker typing" effect
- **Markdown Cleaning**: Automatically extracts code blocks from LLM responses
- **Circular Buffer**: Never runs out of text - loops continuously
- **Clipboard Copy**: Press `Esc` or `Enter` to copy all buffer content to system clipboard
- **File Logging**: Logs written to `sloptyper.log` to avoid interfering with TUI
- **CLI Configuration**: Model, API key, and base URL via command-line arguments

## Architecture

```
src/
├── main.rs              # Entry point, CLI parsing, HTTP server, TUI startup
├── config.rs            # CLI configuration (clap)
├── buffer_manager.rs    # Code buffer management, circular buffer, batch extraction
├── llm_stream.rs        # Token streaming (mock mode), code block extraction
├── http_server.rs       # HTTP server for receiving code from AI agents (axum) - legacy module
└── keyboard_hook.rs     # TUI event loop, raw mode input, crossterm rendering
```

## How It Works

1. SlopTyper starts as a TUI application in raw mode
2. HTTP server runs on `127.0.0.1:8765` in background
3. AI agents send generated text to `POST /stream`
4. Server extracts code blocks from markdown and loads clean code into buffer
5. TUI is active immediately - press any key to inject code from buffer
6. Each key press outputs 5-8 characters in green
7. Press `Esc` or `Enter` to copy entire buffer to clipboard and exit

## Prerequisites

- **Rust 1.75+** (install via [rustup](https://rustup.rs/))
- **Windows**: No additional deps
- **Linux**: Standard terminal (works in any TTY/terminal emulator)
- **macOS**: Standard terminal

## Build Instructions

### Debug Build
```bash
cargo build
```

### Release Build (Recommended)
```bash
cargo build --release
```
The binary will be at `target/release/sloptyper` (or `sloptyper.exe` on Windows).

### Run Directly
```bash
cargo run --release
```

## Usage

### Starting the Application

```bash
# With defaults
./target/release/sloptyper

# With custom model
./target/release/sloptyper --model gpt-4

# With API key
./target/release/sloptyper --api-key sk-xxx

# With custom base URL (for Ollama/LocalLLM)
./target/release/sloptyper --base-url http://localhost:11434/v1

# Combined
./target/release/sloptyper --model deepseek-chat --api-key sk-xxx --base-url https://api.deepseek.com
```

### CLI Options

| Option | Default | Description |
|--------|---------|-------------|
| `--model` | `deepseek-chat` | Model name for LLM API |
| `--api-key` | (from `OPENAI_API_KEY` env) | API key for LLM service |
| `--base-url` | (none) | Base URL for API (e.g., Ollama, LocalAI) |

### Sending Code from AI Agents

Send code to SlopTyper's HTTP server:

**POST /stream** with JSON body:
```json
{
  "text": "```rust\nfn hello() {\n    println!(\"Hello, world!\");\n}\n```"
}
```

Or send raw text directly:
```
POST /stream
Content-Type: text/plain

fn hello() {
    println!("Hello, world!");
}
```

The server will:
1. Extract code blocks from markdown (if present)
2. Load clean code into the internal buffer

### Using the TUI

1. **Start** SlopTyper: `cargo run --release`
2. **Send code** via `POST http://127.0.0.1:8765/stream` (or it loads sample code on startup)
3. **Type in the TUI** - any key press injects code in green
4. **Exit & Copy** - press `Esc` or `Enter` to copy full buffer to clipboard and quit

## Configuration

All configuration via CLI arguments (see above). No config file needed.

## Code Block Extraction

The `extract_code_blocks(text: &str) -> String` function in `llm_stream.rs`:
- Detects code blocks delimited by triple backticks (```)
- Extracts only the content inside code blocks (removes markdown formatting and language specifiers)
- Handles streaming tokens by buffering until complete code blocks are available
- If no code blocks are found, returns the text as-is

## Troubleshooting

### TUI exits immediately
- Ensure you're running in a proper terminal (not IDE output pane)
- Check that `sloptyper.log` has any error messages

### Clipboard not working
- **Windows**: Should work out of the box
- **Linux**: Requires `xclip` or `xsel` or `wl-clipboard` (Wayland)
- **macOS**: Should work out of the box

### HTTP server not accessible
- Ensure port 8765 is not blocked by firewall
- Check `sloptyper.log` for bind errors

## License

MIT
