# Veda Streaming Architecture

Veda has been redesigned to use Claude Code's streaming JSON output format for real-time, interactive communication with multiple Claude instances.

## Architecture Overview

```
┌─────────────────────────────────────────────┐
│                 Veda TUI                     │
│  ┌─────────┬─────────┬─────────┐           │
│  │ Tab 1   │ Tab 2   │ Tab 3   │           │
│  │ Claude  │ Claude  │ Claude  │           │
│  └─────────┴─────────┴─────────┘           │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│            Claude Manager                    │
│  • Message queuing                          │
│  • Stream parsing                           │
│  • Process management                       │
└─────────────────────────────────────────────┘
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
┌──────────┐ ┌──────────┐ ┌──────────┐
│ Claude   │ │ Claude   │ │ Claude   │
│ Process  │ │ Process  │ │ Process  │
│          │ │          │ │          │
│ --output-│ │ --output-│ │ --output-│
│  format  │ │  format  │ │  format  │
│  stream- │ │  stream- │ │  stream- │
│   json   │ │   json   │ │   json   │
└──────────┘ └──────────┘ └──────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│          Ollama Decision Engine              │
│         (deepseek-r1:14b)                    │
│  • Analyzes Claude output                    │
│  • Makes automated decisions                 │
│  • Responds when appropriate                 │
└─────────────────────────────────────────────┘
```

## How It Works

### 1. Message Sending

When you type a message in the TUI, Veda executes:
```bash
claude -p "your message" --output-format stream-json
```

### 2. Stream Parsing

The streaming JSON output is parsed in real-time:

```json
{"type": "message_start"}
{"type": "content_block_start"}
{"type": "content_block_delta", "delta": {"text": "I'll help you..."}}
{"type": "content_block_delta", "delta": {"text": " with that."}}
{"type": "content_block_stop"}
{"type": "message_stop"}
```

Each JSON event is processed immediately:
- Text deltas are displayed character-by-character
- Tool use events are handled appropriately
- Errors are shown in the UI

### 3. Auto-Response System

When auto-mode is enabled (Ctrl+D), the Ollama decision engine:

1. Monitors Claude's complete messages
2. Analyzes context using deepseek-r1:14b
3. Determines if auto-response is appropriate
4. Sends responses automatically when needed

Example decision flow:
```python
# Claude asks: "Should I create this file? (y/n)"
# Ollama analyzes and responds: "y"
# Veda automatically sends "y" to Claude
```

### 4. Process Management

Each Claude instance runs as a separate subprocess:
- Independent conversation contexts
- Isolated working directories (optional)
- Clean process lifecycle management

## Key Components

### ClaudeStreamParser
- Parses streaming JSON line by line
- Maintains message state
- Handles all Claude event types

### ClaudeInstance
- Manages subprocess lifecycle
- Queues messages
- Tracks conversation history
- Uses `-c` flag for session continuity

### ClaudeOrchestrator
- Coordinates multiple instances
- Routes messages to UI
- Manages auto-response logic

### VedaTUI
- Textual-based interface
- Tab management
- Real-time display updates
- Word wrapping for all output
- Responsive layout

## Benefits

1. **Real-time Feedback**: See Claude's responses as they're generated
2. **True Interactivity**: Interrupt, redirect, or clarify at any time
3. **Parallel Conversations**: Manage multiple Claude instances simultaneously
4. **Automated Workflow**: Let AI handle routine decisions
5. **Clean Architecture**: Proper separation of concerns
6. **Session Persistence**: Each Claude instance maintains conversation context with `-c` flag
7. **Responsive Display**: Word wrapping adapts to terminal width

## Example Usage

```python
# Start Veda with 3 instances
$ veda -n 3

# In Instance 0
You: Build a REST API for user management
Claude: I'll help you build a REST API for user management. Let me start by...

# In Instance 1
You: Create unit tests for the API
Claude: I'll create comprehensive unit tests for your API...

# Enable auto-mode (Ctrl+D)
Claude: Should I create the test file? (y/n)
[AUTO] y
Claude: Creating test_api.py...
```

This architecture provides a robust foundation for managing multiple AI coding assistants with real-time interaction and intelligent automation.