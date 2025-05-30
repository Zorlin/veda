# Veda TUI Automode

## Overview
Automode is a feature that automatically intercepts Claude's questions and uses DeepSeek-R1:8b (via Ollama) to provide guidance and suggest using the deepwiki MCP plugin for documentation lookups. It also automatically enables tools when Claude mentions permission issues.

## How it Works
1. When automode is enabled (Ctrl+A), the TUI monitors Claude's responses
2. Tool use attempts are displayed in the UI with a 🔧 icon in magenta
3. If Claude attempts to use a tool AND THEN mentions it cannot use it:
   - DeepSeek analyzes which tools need permission
   - The TUI automatically runs `claude config add allowedTools <tool>` for each tool
   - Claude is informed the tools are now enabled
4. If Claude asks a question (without tool permission issues), DeepSeek-R1:8b is automatically invoked to:
   - Analyze the question
   - Provide helpful guidance
   - Suggest using deepwiki MCP tools when documentation is needed

## Prerequisites
- Ollama must be running locally at `http://localhost:11434`
- DeepSeek-R1:8b model must be installed: `ollama pull deepseek-r1:8b`

## Usage
1. Start the TUI: `cargo run`
2. Press Ctrl+A to toggle automode (status shown in title bar)
3. Send messages to Claude as normal
4. When Claude asks questions, DeepSeek will automatically respond

## DeepWiki Integration
When Claude needs documentation, DeepSeek will suggest using:
- `mcp__deepwiki__read_wiki_structure` - List available docs
- `mcp__deepwiki__read_wiki_contents` - Read documentation
- `mcp__deepwiki__ask_question` - Ask specific questions

Example: For React docs, DeepSeek will suggest:
```
mcp__deepwiki__read_wiki_contents {"repoName": "facebook/react"}
```

## Tool Permission Management

The TUI tracks all tool use attempts. When Claude:
1. First attempts to use a tool (shown as "🔧 Attempting to use: tool_name")
2. Then mentions it cannot use that tool

Automode will:
1. Use DeepSeek to identify which specific tools need permission
2. Automatically run `claude config add allowedTools <toolname>` for each tool
3. Send a message to Claude confirming the tools are enabled

This ensures Claude has the necessary permissions to help you effectively.

### UI Indicators
- **Tool attempts**: Displayed in magenta with 🔧 icon
- **Regular messages**: 
  - You: Cyan
  - Claude: Green
  - Errors: Red