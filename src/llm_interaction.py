import json
import logging
import requests
from typing import Dict, Any, List, Optional

# Configure logging for this module
logger = logging.getLogger(__name__)

def get_llm_response(
    prompt: str,
    config: Dict[str, Any],
    history: Optional[List[Dict[str, str]]] = None,
    system_prompt: Optional[str] = None,
) -> str:
    """
    Sends a prompt (with optional history and system prompt) to the configured Ollama LLM
    and returns the response content.

    Args:
        prompt: The user prompt string to send to the LLM.
        config: Dictionary containing Ollama configuration (model, url, options).
        history: Optional list of previous conversation messages.
        system_prompt: Optional system prompt to guide the LLM's behavior.

    Returns:
        The content string of the LLM's response.

    Raises:
        requests.exceptions.RequestException: If the API request fails.
        KeyError: If the expected keys are missing in the response.
        ValueError: If the response indicates an error from Ollama.
    """
    ollama_url = config.get("ollama_api_url", "http://localhost:11434/api/chat")
    # Use the specific model from config, fall back to a default if necessary
    ollama_model = config.get("ollama_model", "deepcoder:14b")
    ollama_options = config.get("ollama_options", {}) # e.g., {"temperature": 0.7}

    messages = []
    if system_prompt:
        messages.append({"role": "system", "content": system_prompt})
    if history:
        messages.extend(history)
    messages.append({"role": "user", "content": prompt})

    payload = {
        "model": ollama_model,
        "messages": messages,
        "stream": False, # We want the full response at once
        "options": ollama_options
    }

    logger.debug(f"Sending request to Ollama URL: {ollama_url}")
    logger.debug(f"Using Ollama model: {ollama_model}")
    # Avoid logging potentially large history/prompt payload at info level
    logger.debug(f"Ollama request payload (messages truncated): "
                 f"model={payload['model']}, "
                 f"messages=[...{len(payload['messages'])} messages...], "
                 f"options={payload['options']}")

    try:
        response = requests.post(ollama_url, json=payload, timeout=180) # Add timeout
        response.raise_for_status()  # Raise HTTPError for bad responses (4xx or 5xx)
        response_data = response.json()

        # Handle potential errors returned in the JSON payload itself
        if response_data.get("error"):
            error_msg = f"Ollama API returned an error: {response_data['error']}"
            logger.error(error_msg)
            raise ValueError(error_msg)

        # Check if the expected keys exist
        if "message" not in response_data or "content" not in response_data["message"]:
            error_msg = f"LLM response missing 'message' or 'content' key. Response: {response_data}"
            logger.error(error_msg)
            raise KeyError(error_msg)

        llm_content = response_data["message"]["content"]
        logger.debug(f"Received LLM response content (truncated): {llm_content[:200]}...")
        return llm_content.strip()

    except requests.exceptions.Timeout:
        logger.error(f"Timeout connecting to Ollama API at {ollama_url}")
        raise requests.exceptions.RequestException("Timeout connecting to Ollama API")
    except requests.exceptions.RequestException as e:
        logger.error(f"Error communicating with Ollama API at {ollama_url}: {e}")
        # Log response text if available
        if hasattr(e, 'response') and e.response is not None:
            logger.error(f"Ollama Response Status Code: {e.response.status_code}")
            logger.error(f"Ollama Response Text: {e.response.text}")
        raise
    except json.JSONDecodeError:
        logger.error(f"Error decoding JSON response from Ollama: {response.text}")
        raise requests.exceptions.RequestException("Invalid JSON response from Ollama")
    except (KeyError, ValueError) as e:
        # Errors raised explicitly above
        raise
    except Exception as e:
        logger.exception(f"An unexpected error occurred during LLM interaction: {e}")
        raise requests.exceptions.RequestException(f"Unexpected LLM interaction error: {e}")


# Example usage (optional, for testing this module directly)
if __name__ == '__main__':
    # Configure basic logging for testing
    logging.basicConfig(level=logging.DEBUG, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    logger.info("Running llm_interaction module test...")

    test_config = {
        "ollama_model": "deepcoder:14b", # Use a model you have running
        "ollama_api_url": "http://localhost:11434/api/chat", # Ensure this matches Ollama setup
        "ollama_options": {"temperature": 0.1}
    }
    test_history = [{"role": "system", "content": "You are a concise assistant."}]
    test_prompt = "What is the capital of France?"

    try:
        llm_reply = get_llm_response(test_prompt, test_config, test_history)
        logger.info(f"Test LLM Prompt: {test_prompt}")
        logger.info(f"Test LLM Response: {llm_reply}")
    except Exception as e:
        logger.error(f"LLM interaction test failed: {e}", exc_info=True)

    # Test error handling (e.g., invalid URL)
    logger.info("Testing error handling (invalid URL)...")
    invalid_config = test_config.copy()
    invalid_config["ollama_api_url"] = "http://invalid-url-that-does-not-exist:11434/api/chat"
    try:
        get_llm_response(test_prompt, invalid_config, test_history)
    except requests.exceptions.RequestException as e:
        logger.info(f"Successfully caught expected RequestException: {e}")
    except Exception as e:
        logger.error(f"Caught unexpected exception during error test: {e}", exc_info=True)
