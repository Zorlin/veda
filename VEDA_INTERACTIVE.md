# Veda Interactive - Multi-Instance Claude Code Orchestrator

Veda provides an interactive TUI (Terminal User Interface) for managing multiple Claude Code instances simultaneously, with automatic decision-making powered by Ollama's deepseek-r1:14b model.

## Features

- **Interactive Multi-Instance Management**: Spawn and manage 1-10 Claude Code instances
- **Real-Time Streaming Output**: See what each instance is doing in real-time
- **Tab-Based Navigation**: Switch between instances using arrow keys
- **Auto-Decision Mode**: Let deepseek-r1:14b handle Claude's questions automatically
- **Bidirectional Communication**: Chat back and forth with each instance
- **Perpetual Operation**: Instances keep running with AI-driven decisions

## Installation

```bash
./setup.sh
```

Requirements:
- Claude Code CLI (`claude` command)
- Ollama with deepseek-r1:14b model
- Python 3.10+ with textual, aiohttp, pyyaml

## Usage

### Basic Usage

```bash
# Start with a single instance
veda

# Start with multiple instances
veda -n 3

# Start with 5 instances
veda -n 5
```

### Interactive Controls

- **Left/Right Arrow Keys**: Switch between instance tabs
- **Enter**: Send input to current instance
- **Ctrl+D**: Toggle auto-decision mode (AI handles Claude's questions)
- **Ctrl+N**: Create a new instance
- **Ctrl+C**: Quit

### Auto-Decision Mode

When enabled (Ctrl+D), Veda uses deepseek-r1:14b to automatically:
- Approve file changes when appropriate
- Answer yes/no questions
- Provide inputs Claude needs
- Make decisions based on context

Example auto-decisions:
- "Do you want to create this file? (y/n)" → "y"
- "Continue with these changes?" → "yes"
- "Which option? (1/2/3)" → Analyzes context and chooses

### Interface Layout

```
┌─────────────────────────────────────────────────┐
│ Veda Interactive                                 │
├─────────────────────────────────────────────────┤
│ [Instance 0] [Instance 1] [Instance 2]          │
├─────────────────────────────────────────────────┤
│ Claude Instance 0 - /Users/you/project          │
│ ┌─────────────────────────────────────────────┐ │
│ │ Claude: I'll help you build a web scraper.  │ │
│ │ Let me start by...                          │ │
│ │                                             │ │
│ │ Do you want to create scraper.py? (y/n)    │ │
│ │ [AUTO-DECISION: y]                          │ │
│ └─────────────────────────────────────────────┘ │
│ Input (Enter to send, Ctrl+D for auto):         │
│ ┌─────────────────────────────────────────────┐ │
│ │                                             │ │
│ └─────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

## Workflow Example

1. **Start Veda**:
   ```bash
   veda -n 3  # Start with 3 instances
   ```

2. **Interact with each instance**:
   - Instance 0: Ask to work on backend API
   - Instance 1: Request frontend components
   - Instance 2: Have it set up authentication

3. **Enable auto-decision mode** (Ctrl+D):
   - deepseek-r1:14b handles routine decisions
   - You focus on important design choices

4. **Switch between instances** (arrow keys):
   - Monitor progress
   - Provide specific guidance when needed
   - Let AI handle the routine work

5. **Give each instance tasks**:
   ```
   # In Instance 0
   > Build a REST API with user management
   
   # In Instance 1  
   > Create a React frontend with login page
   
   # In Instance 2
   > Set up JWT authentication
   ```

## Architecture

```
User Input → Veda TUI
                ↓
        ┌───────┴───────┐
        │   Instance    │
        │   Manager     │
        └───────┬───────┘
                ↓
    ┌───────────┼───────────┐
    ↓           ↓           ↓
Instance 0  Instance 1  Instance 2
(Claude)    (Claude)    (Claude)
    ↑           ↑           ↑
    └───────────┴───────────┘
                ↓
         Ollama Decision
         (deepseek-r1:14b)
```

## Tips

- Use auto-decision mode for routine tasks
- Manually intervene for critical decisions
- Create new instances (Ctrl+N) for subtasks
- Each instance maintains its own context
- Instances can coordinate via Task Master AI

## Configuration

### Ollama Setup
```bash
# Ensure Ollama is running
ollama serve

# Pull the model if not already installed
ollama pull deepseek-r1:14b
```

### MCP Tools
Instances have access to all MCP tools in `.mcp.json`:
- DeepWiki for research
- Task Master AI for coordination
- Playwright for browser automation