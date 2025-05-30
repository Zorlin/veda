# Veda TUI - Rust Implementation

A terminal user interface for managing Claude instances, written in Rust using Ratatui.

## Features

- **Multiple Claude Instances**: Manage multiple Claude conversations in separate tabs
- **Text Selection**: Mouse-based text selection with clipboard support
- **Streaming Responses**: Real-time streaming of Claude's responses using `--output-format stream-json`
- **Tool Use Tracking**: Visual indicators when Claude attempts to use tools
- **Automode**: Automatically uses DeepSeek-R1:8b to answer Claude's questions and enable tools when needed (ON by default)
- **Keyboard Shortcuts**:
  - `Ctrl+C` or `ESC`: Quit (Ctrl+C copies text if selected)
  - `Ctrl+N`: Create new Claude instance
  - `Ctrl+A`: Toggle automode (shown as `[Auto: ON/OFF]` in UI)
  - `Ctrl+←/→`: Navigate between tabs
  - `Enter`: Send message

## Prerequisites

- Rust 1.70 or later
- Claude CLI installed and configured (`claude` command available in PATH)
- For automode: Ollama running with DeepSeek-R1:8b model (`ollama pull deepseek-r1:8b`)

## Installation

```bash
# Clone the repository
cd /path/to/veda/veda-tui-rust

# Build the project
cargo build --release

# Run the TUI
cargo run
```

## Usage

1. Launch the application:
   ```bash
   cargo run
   ```

2. Type your message in the input area and press Enter to send to Claude

3. Select text with mouse drag to copy to clipboard

4. Use keyboard shortcuts to navigate and manage instances

## Architecture

- `src/main.rs`: Main TUI application logic using Ratatui
- `src/claude.rs`: Claude process management and JSON streaming
- `src/deepseek.rs`: DeepSeek integration via Ollama API for automode
- `src/lib.rs`: Shared data structures for testing

## Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture
```

## Differences from Python Version

- **Native Performance**: Faster rendering and lower resource usage
- **Better Text Selection**: Native mouse selection support
- **Type Safety**: Compile-time guarantees and better error handling
- **Async/Await**: Modern async handling for Claude processes

## Development

The project uses:
- `ratatui` for TUI framework
- `crossterm` for terminal handling
- `tokio` for async runtime
- `arboard` for clipboard support
- `serde` for JSON parsing

## Automode

When automode is enabled, the TUI will:
1. Display tool use attempts with 🔧 icon in magenta
2. Monitor if Claude reports permission problems after attempting to use tools
3. Automatically enable tools when Claude can't use them
4. Use DeepSeek-R1:8b to answer questions and suggest documentation tools

See [AUTOMODE.md](AUTOMODE.md) for more details.

## License

MIT