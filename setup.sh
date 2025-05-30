#!/bin/bash

# Setup script for Veda - Claude Code Orchestrator

echo "Setting up Veda as a Claude Code orchestrator..."

# Install Veda package
pip install -e .

echo "Installed veda command"

# Check for Claude Code installation
if ! command -v claude &> /dev/null; then
    echo "Warning: Claude Code not found. Install from https://claude.ai/download"
fi

# Check for Ollama installation
if ! command -v ollama &> /dev/null; then
    echo "Warning: Ollama not found. Install from https://ollama.ai"
    echo "After installing, run: ollama pull deepseek-r1:14b"
else
    # Check if deepseek-r1:14b is installed
    if ! ollama list | grep -q "deepseek-r1:14b"; then
        echo "Pulling deepseek-r1:14b model..."
        ollama pull deepseek-r1:14b
    fi
fi

# Check for npm (needed for MCP tools)
if ! command -v npm &> /dev/null; then
    echo "Warning: npm not found. Install Node.js to use MCP tools"
fi

echo "Setup complete! You can now use:"
echo "  veda -p 'your task'"
echo "  veda -p 'complex task' -n 3  # Force 3 instances"
echo "  veda -p 'analyze this' --analyze-only"