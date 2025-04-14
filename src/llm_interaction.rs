use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, instrument};

use crate::constants; // For OLLAMA_URL and VEDA_CHAT_MODEL

// Structures matching Ollama's /api/generate endpoint
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool, // We want the full response, not a stream
    options: Option<serde_json::Value>, // Optional parameters like temperature
}

#[derive(Deserialize, Debug)]
struct OllamaResponse {
    model: String,
    created_at: String,
    response: String, // The generated text
    done: bool,
    // Other fields like context, timings, etc., are ignored for now
}

#[instrument(skip(tags))]
pub async fn synthesize_goal_with_ollama(tags: Vec<String>) -> Result<String> {
    if tags.is_empty() {
        return Ok("".to_string()); // Return empty if no tags provided
    }

    let client = Client::new();
    let model_name = constants::VEDA_CHAT_MODEL.clone(); // Use the configured chat model
    let ollama_api_url = format!("{}/api/generate", *constants::OLLAMA_URL);

    // Construct the prompt for the LLM
    let tag_list = tags
        .iter()
        .map(|tag| format!("- {}", tag))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Combine the following short goals or tasks into a single, coherent project goal statement. \
        Focus on clarity and conciseness. Present *only* the final synthesized goal statement, \
        without any preamble, introduction, or explanation.\n\nTasks:\n{}\n\nSynthesized Goal:",
        tag_list
    );

    debug!(?prompt, "Constructed Ollama prompt for goal synthesis");

    let request_payload = OllamaRequest {
        model: model_name.clone(),
        prompt,
        stream: false,
        options: None, // Add options like temperature if needed
    };

    let response = client
        .post(&ollama_api_url)
        .json(&request_payload)
        .send()
        .await
        .context(format!("Failed to send request to Ollama API at {}", ollama_api_url))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_else(|_| "Failed to read error body".to_string());
        error!(%status, %error_body, "Ollama API request failed");
        return Err(anyhow::anyhow!(
            "Ollama API request failed with status {}: {}",
            status, error_body
        ));
    }

    let ollama_response = response
        .json::<OllamaResponse>()
        .await
        .context("Failed to parse JSON response from Ollama API")?;

    debug!(response = ?ollama_response.response, "Received Ollama response");

    // Return the synthesized goal, trimming whitespace
    Ok(ollama_response.response.trim().to_string())
}
