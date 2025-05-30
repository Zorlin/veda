# Veda - Claude Code Orchestrator

Veda orchestrates multiple Claude Code instances to work on complex tasks, using Ollama's deepseek-r1:14b model for intelligent task analysis and coordination.

## Features

- **Multi-Instance Orchestration**: Spawn 1-10 Claude Code instances
- **Intelligent Task Analysis**: Uses deepseek-r1:14b to analyze tasks and determine optimal approach
- **Flexible Work Modes**:
  - Same worktree with Task Master AI coordination
  - Separate git worktrees for parallel independent work
- **MCP Tool Integration**: Emphasizes DeepWiki and other MCP tools
- **Stream JSON Output**: Real-time monitoring of all instances

## Installation

```bash
./setup.sh
```

Requirements:
- Claude Code CLI (`claude` command)
- Ollama with deepseek-r1:14b model
- Python 3.7+ with aiohttp and pyyaml
- Node.js/npm for MCP tools

## Usage

### Basic Usage

```bash
# Simple task - Veda will analyze and decide how many instances needed
veda -p "implement authentication system with tests"

# Force specific number of instances
veda -p "refactor the entire codebase" -n 5

# Analyze only (see what Veda would do without running)
veda -p "build a web scraper" --analyze-only
```

### How It Works

1. **Task Analysis**: Veda uses deepseek-r1:14b to analyze your task and determine:
   - Number of Claude instances needed (1-10)
   - Whether to use shared worktree or separate worktrees
   - Which MCP tools to emphasize
   - How to break down the work

2. **Instance Coordination**:
   - **Shared Worktree**: Instances use Task Master AI to coordinate
   - **Separate Worktrees**: Each instance works independently in its own git branch

3. **MCP Tools**: Instances are instructed to use available MCP tools, especially:
   - DeepWiki for documentation and research
   - Task Master AI for coordination
   - Playwright for browser automation

### Example Scenarios

```bash
# Research task - likely uses 1-2 instances with heavy DeepWiki usage
veda -p "research best practices for React performance optimization"

# Complex feature - multiple instances with Task Master coordination
veda -p "add real-time collaboration to our editor"

# Large refactor - separate worktrees for parallel work
veda -p "migrate from JavaScript to TypeScript"
```

### Output

Veda provides real-time output from all instances:
```
🤔 Analyzing task with deepseek-r1:14b...

📋 Analysis complete:
  - Instances needed: 3
  - Use worktrees: false
  - MCP tools: deepwiki, taskmaster-ai
  - Strategy: Coordinate through Task Master AI for shared context

🚀 Started 3 Claude Code instance(s)

[Instance 0] Initializing Task Master AI...
[Instance 1] Researching authentication patterns with DeepWiki...
[Instance 2] Setting up test framework...
```

## Configuration

### MCP Tools (.mcp.json)

Veda reads your project's `.mcp.json` to make tools available to Claude instances:

```json
{
  "mcpServers": {
    "deepwiki": {
      "type": "sse",
      "url": "https://mcp.deepwiki.com/sse"
    },
    "taskmaster-ai": {
      "command": "npx",
      "args": ["-y", "--package=task-master-ai", "task-master-ai"]
    }
  }
}
```

### Ollama Configuration

Ensure Ollama is running and has the deepseek-r1:14b model:
```bash
ollama serve  # Start Ollama server
ollama pull deepseek-r1:14b  # Download model
```

## Architecture

```
veda -p "task"
    ↓
OllamaCoordinator (deepseek-r1:14b)
    ↓
Task Analysis & Strategy
    ↓
ClaudeInstance(s) Creation
    ├─ Instance 0 (coordinator)
    ├─ Instance 1 (worker)
    └─ Instance 2 (worker)
         ↓
    Task Completion
```

## Logs

Each Claude instance creates a log file:
- `veda_instance_0.log`
- `veda_instance_1.log`
- etc.

These contain the full JSON stream output for debugging and analysis.