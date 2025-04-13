import os

# Default Ollama server URL (can be overridden by OLLAMA_URL env var)
OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://10.7.1.200:11434")

# Default model for Veda chatbot
VEDA_CHAT_MODEL = "gemma3:12b"

# Role-specific models
ROLE_MODELS = {
    "theorist": "qwen2.5:14b",
    "architect": "deepcoder:14b",
    "skeptic": "gemma3:12b",
    "historian": "qwen2.5:14b",
    "coordinator": "command-r7b",
}

# MCP (RAG) server URL (can be overridden by MCP_URL env var)
MCP_URL = os.environ.get("MCP_URL", "http://localhost:8001")

# Knowledge base config
POSTGRES_DSN = os.environ.get("VEDA_PG_DSN", "postgresql://veda:veda@localhost:5432/veda")
HANDOFF_DIR = os.environ.get("VEDA_HANDOFF_DIR", "handoffs")
