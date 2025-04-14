// Placeholder for constants, potentially loaded from environment or config files.

use std::env;

// Example: Load Ollama URL from environment or use default
// Use lazy_static to initialize static variables safely.
lazy_static::lazy_static! {
    pub static ref OLLAMA_URL: String = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    // TODO: Add other constants like VEDA_CHAT_MODEL, ROLE_MODELS, etc.
    //       These should align with the Python constants initially.
    pub static ref VEDA_CHAT_MODEL: String = env::var("VEDA_CHAT_MODEL").unwrap_or_else(|_| "gemma3:12b".to_string()); // Example
    pub static ref OPENROUTER_API_KEY: String = env::var("OPENROUTER_API_KEY").unwrap_or_default();
    // Add more constants as needed from src/constants.py
    pub static ref POSTGRES_DSN: String = env::var("VEDA_PG_DSN").unwrap_or_else(|_| "postgresql://veda:veda@localhost:5432/veda".to_string());
    pub static ref HANDOFF_DIR: String = env::var("VEDA_HANDOFF_DIR").unwrap_or_else(|_| "handoffs".to_string());
    pub static ref AIDER_PRIMARY_MODEL: String = env::var("AIDER_PRIMARY_MODEL").unwrap_or_else(|_| "openrouter/google/gemini-2.5-pro-exp-03-25:free".to_string());
}

// Note: You already added lazy_static = "1.4.0" to Cargo.toml
