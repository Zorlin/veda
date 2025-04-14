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
    #[serde(skip_serializing_if = "Option::is_none")] // Omit if None
    options: Option<serde_json::Value>, // Optional parameters like temperature
}

#[derive(Deserialize, Debug)]
struct OllamaResponse {
    // Prefix unused fields with _ to silence warnings
    _model: String,
    _created_at: String,
    response: String, // The generated text
    _done: bool,
    // Other fields like context, timings, etc., are ignored for now
}

// Modify signature to accept base URL
#[instrument(skip(tags, ollama_api_base_url))]
pub async fn synthesize_goal_with_ollama(tags: Vec<String>, ollama_api_base_url: &str) -> Result<String> {
    if tags.is_empty() {
        return Ok("".to_string()); // Return empty if no tags provided
    }

    let client = Client::new();
    let model_name = constants::VEDA_CHAT_MODEL.clone(); // Use the configured chat model
    // Construct full URL from base URL argument
    let ollama_api_url = format!("{}/api/generate", ollama_api_base_url.trim_end_matches('/'));


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
        // Revert to None, as this is simpler and likely serialized correctly by reqwest
        options: None,
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


#[cfg(test)]
mod tests {
    use super::*;
    // Ensure body_json import is present
    use wiremock::matchers::{method, path, body_json};
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use serde_json::json;
    // Remove unused test_log::test
    // use test_log::test;

    #[tokio::test]
    async fn test_synthesize_goal_success() {
        // Arrange
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri(); // Use the variable again
        // let _lock = constants::OLLAMA_URL.set(mock_uri); // Removed override

        let tags = vec!["tag1".to_string(), "tag2".to_string()];
        let expected_prompt = "Combine the following short goals or tasks into a single, coherent project goal statement. Focus on clarity and conciseness. Present *only* the final synthesized goal statement, without any preamble, introduction, or explanation.\n\nTasks:\n- tag1\n- tag2\n\nSynthesized Goal:";
        let expected_model = constants::VEDA_CHAT_MODEL.clone();

        // Define the fields we *definitely* expect for body_partial_json
        let expected_partial_body = json!({
            "model": expected_model,
            "prompt": expected_prompt,
            "stream": false,
        });

        let mock_response_body = json!({
            "model": expected_model,
            "created_at": "2023-10-26T18:00:00Z",
            "response": " Synthesized goal from tag1 and tag2. ", // Note leading/trailing spaces
            "done": true,
            "context": [1, 2, 3], // Example context
            "total_duration": 1000000000,
            "load_duration": 1000000,
            "prompt_eval_count": 10,
            "prompt_eval_duration": 500000000,
            "eval_count": 5,
            "eval_duration": 400000000
        });

        Mock::given(method("POST"))
            .and(path("/api/generate"))
            // Use body_partial_json matcher
            .and(wiremock::matchers::body_partial_json(&expected_partial_body))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response_body))
            .mount(&mock_server)
            .await;

        // Act - Pass the mock server URI to the function
        let result = synthesize_goal_with_ollama(tags, &mock_uri).await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Synthesized goal from tag1 and tag2."); // Check trimming
        mock_server.verify().await; // Ensure the mock was called
    }

    #[tokio::test]
    async fn test_synthesize_goal_empty_tags() {
        // Arrange
        let tags = Vec::<String>::new();
        let dummy_url = "http://dummy-url-for-empty-test"; // Provide a dummy URL

        // Act
        let result = synthesize_goal_with_ollama(tags, dummy_url).await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

     #[tokio::test]
    async fn test_synthesize_goal_ollama_error() {
        // Arrange
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();
        // let _lock = constants::OLLAMA_URL.set(mock_uri); // Removed override

        let tags = vec!["tag1".to_string()];

        Mock::given(method("POST"))
            .and(path("/api/generate"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        // Act - Pass the mock server URI
        let result = synthesize_goal_with_ollama(tags, &mock_uri).await;

        // Assert
        assert!(result.is_err());
        let error_message = result.err().unwrap().to_string();
        assert!(error_message.contains("Ollama API request failed with status 500 Internal Server Error"));
        assert!(error_message.contains("Internal Server Error"));
        mock_server.verify().await;
    }

     #[tokio::test]
    async fn test_synthesize_goal_network_error() {
         // Arrange - No mock server running at this address
         let invalid_uri = "http://127.0.0.1:1"; // Use a port very unlikely to be open
         // let _lock = constants::OLLAMA_URL.set(invalid_uri); // Removed override

         let tags = vec!["tag1".to_string()];

         // Act - Pass the invalid URI
         let result = synthesize_goal_with_ollama(tags, invalid_uri).await;

         // Assert
         assert!(result.is_err());
         // Check for a connection error specifically if possible, otherwise just that it failed to send
         let err_string = result.err().unwrap().to_string();
         assert!(err_string.contains("Failed to send request") || err_string.contains("error sending request"));
     }

     // NOTE: Unsafe constant override helpers moved outside this test module.
}
