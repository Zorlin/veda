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


#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, body_json};
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use serde_json::json;
    use test_log::test; // Enables logging during tests

    #[tokio::test]
    async fn test_synthesize_goal_success() {
        // Arrange
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();
        // Override the OLLAMA_URL for this test scope
        let _lock = constants::OLLAMA_URL.set(mock_uri); // Requires constants to be mutable or use a test-specific setup

        let tags = vec!["tag1".to_string(), "tag2".to_string()];
        let expected_prompt = "Combine the following short goals or tasks into a single, coherent project goal statement. Focus on clarity and conciseness. Present *only* the final synthesized goal statement, without any preamble, introduction, or explanation.\n\nTasks:\n- tag1\n- tag2\n\nSynthesized Goal:";
        let expected_model = constants::VEDA_CHAT_MODEL.clone();

        let mock_request_body = json!({
            "model": expected_model,
            "prompt": expected_prompt,
            "stream": false,
            "options": null // Ensure options match if you add them later
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
            .and(body_json(&mock_request_body)) // Match the exact request body
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response_body))
            .mount(&mock_server)
            .await;

        // Act
        let result = synthesize_goal_with_ollama(tags).await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Synthesized goal from tag1 and tag2."); // Check trimming
        mock_server.verify().await; // Ensure the mock was called
    }

    #[tokio::test]
    async fn test_synthesize_goal_empty_tags() {
        // Arrange
        let tags = Vec::<String>::new();

        // Act
        let result = synthesize_goal_with_ollama(tags).await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

     #[tokio::test]
    async fn test_synthesize_goal_ollama_error() {
        // Arrange
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();
        let _lock = constants::OLLAMA_URL.set(mock_uri);

        let tags = vec!["tag1".to_string()];

        Mock::given(method("POST"))
            .and(path("/api/generate"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        // Act
        let result = synthesize_goal_with_ollama(tags).await;

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
         let invalid_uri = "http://127.0.0.1:1".to_string(); // Use a port very unlikely to be open
         let _lock = constants::OLLAMA_URL.set(invalid_uri);

         let tags = vec!["tag1".to_string()];

         // Act
         let result = synthesize_goal_with_ollama(tags).await;

         // Assert
         assert!(result.is_err());
         assert!(result.err().unwrap().to_string().contains("Failed to send request to Ollama API"));
     }

    // Helper to allow modifying lazy_static constants in tests.
    // This requires careful handling as it modifies global state.
    // Consider using dependency injection for OLLAMA_URL in real applications.
    impl constants::OLLAMA_URL {
        fn set(&'static self, value: String) -> impl Drop {
            // This is a simplified approach. A real implementation might need a Mutex
            // or a more sophisticated test setup (like one-time initialization).
            // For now, we rely on tests running somewhat serially or accepting potential races.
            // A better approach is dependency injection.
            let original = self.as_str().to_string();
            unsafe {
                // SAFETY: Modifying static mut requires unsafe. This is generally discouraged.
                // This is a hack for testing lazy_static.
                let ptr = &**self as *const String as *mut String;
                *ptr = value;
            }
            StaticGuard { original }
        }
    }

    struct StaticGuard {
        original: String,
    }

    impl Drop for StaticGuard {
        fn drop(&mut self) {
            unsafe {
                let ptr = &*constants::OLLAMA_URL as *const String as *mut String;
                *ptr = self.original.clone();
            }
        }
    }
}
