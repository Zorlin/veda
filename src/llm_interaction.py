import logging
import ollama # Use the official ollama library
from ollama import ResponseError # Import specific error type
from requests.exceptions import ConnectionError # Still possible if server is down
from typing import Dict, Any, List, Optional
import json # Keep for potential JSON parsing errors if needed, though less likely

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
        ollama.ResponseError: If the Ollama API returns an error (e.g., model not found).
        requests.exceptions.ConnectionError: If the connection to the Ollama server fails.
        Exception: For other unexpected errors during the interaction.
    """
    # Use the specific model from config, fall back to a default if necessary
    ollama_model = config.get("ollama_model", "gemma3:12b") # Updated default
    ollama_options = config.get("ollama_options", {}).copy() # Copy to avoid modifying original
    request_timeout = config.get("ollama_request_timeout", 300) # Default 5 minutes
    # ollama_host = config.get("ollama_host", None) # Optional: configure host if not default localhost

    messages = []
    if system_prompt:
        messages.append({"role": "system", "content": system_prompt})
    if history:
        messages.extend(history)
    messages.append({"role": "user", "content": prompt})

    logger.debug(f"Sending request to Ollama client")
    logger.debug(f"Using Ollama model: {ollama_model}")
    logger.debug(f"Ollama options: {ollama_options}")
    logger.debug(f"Messages (count: {len(messages)}): {messages}") # Log full messages at debug

    try:
        # Initialize client - host can be configured via OLLAMA_HOST env var or passed explicitly
        # client = ollama.Client(host=ollama_host) # Example if using explicit host config
        client = ollama.Client() # Uses default host (http://localhost:11434) or OLLAMA_HOST env var

        response_data = client.chat(
            model=ollama_model,
            messages=messages,
            stream=False, # We want the full response at once
            options=ollama_options,
            # Add keep_alive handling if needed via options or separate call? Check library docs.
            # Note: The ollama library itself might handle timeouts differently.
            # We pass it via options if supported, otherwise rely on underlying http client timeouts.
            # Checking ollama-python source, timeout isn't a direct param to chat,
            # but the underlying httpx client has timeouts. Let's rely on that for now,
            # but keep the config option for potential future use or explicit client setup.
            # If hangs persist, we might need to configure the client explicitly:
            # client = ollama.Client(timeout=request_timeout)
        )

        # The ollama library response structure is slightly different
        # It raises ResponseError for API errors (like model not found, loading errors)
        # Successful response structure: {'model': '...', 'created_at': '...', 'message': {'role': 'assistant', 'content': '...'}, ...}

        if "message" not in response_data or "content" not in response_data.get("message", {}):
            error_msg = f"LLM response missing 'message.content'. Response: {response_data}"
            logger.error(error_msg)
            raise KeyError(error_msg) # Raise KeyError if structure is unexpected

        llm_content = response_data["message"]["content"]
        logger.debug(f"Received LLM response content (truncated): {llm_content[:200]}...")
        return llm_content.strip()

    except ResponseError as e:
        # Handle specific Ollama errors (like model not found, loading issues)
        error_msg = f"Ollama API error: {e.status_code} - {e.error}"
        # Check if it's the 'model is loading' error specifically
        if "model" in e.error and "is loading" in e.error:
             error_msg = f"Ollama model '{ollama_model}' is still loading or failed to load (ResponseError)."
        logger.error(error_msg)
        raise # Re-raise the specific error for the harness to catch

    except ConnectionError as e:
        # Handle connection errors (Ollama server not running?)
        logger.error(f"Error connecting to Ollama server: {e}")
        raise # Re-raise for the harness

    except Exception as e:
        # Catch any other unexpected errors
        logger.exception(f"An unexpected error occurred during LLM interaction: {e}")
        raise # Re-raise the generic exception


# Example usage (optional, for testing this module directly)
if __name__ == '__main__':
    # Configure basic logging for testing
    logging.basicConfig(level=logging.DEBUG, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    logger.info("Running llm_interaction module test...")

    # Ensure Ollama is running and the model exists for this test
    test_model = "llama3" # Use a common, small model likely available
    try:
        ollama.list() # Check connection
        logger.info(f"Attempting to use model '{test_model}' for testing...")
        # Pre-pull the model if it doesn't exist? Or just let the test fail?
        # ollama.pull(test_model) # Uncomment to ensure model exists

        test_config = {
            "ollama_model": test_model,
            "ollama_options": {"temperature": 0.1}
        }
        test_history = [{"role": "system", "content": "You are a concise assistant."}]
        test_prompt = "What is the capital of France?"

        llm_reply = get_llm_response(test_prompt, test_config, test_history)
        logger.info(f"Test LLM Prompt: {test_prompt}")
        logger.info(f"Test LLM Response: {llm_reply}")

        # Test system prompt
        logger.info("Testing system prompt...")
        sys_prompt = "Respond in Spanish."
        llm_reply_es = get_llm_response(test_prompt, test_config, test_history, system_prompt=sys_prompt)
        logger.info(f"Test LLM Response (Spanish): {llm_reply_es}")
        # Basic check - this is language dependent and might fail
        # assert "parís" in llm_reply_es.lower()

    except ConnectionError:
         logger.error("Ollama connection failed. Is Ollama running?")
    except ResponseError as e:
         logger.error(f"Ollama API error during test: {e.status_code} - {e.error}. Ensure model '{test_model}' is available.")
    except Exception as e:
        logger.error(f"LLM interaction test failed unexpectedly: {e}", exc_info=True)

    # Test error handling (e.g., model not found)
    logger.info("Testing error handling (model not found)...")
    invalid_config = test_config.copy()
    invalid_config["ollama_model"] = "model-that-does-not-exist-hopefully"
    try:
        get_llm_response(test_prompt, invalid_config, test_history)
    except ResponseError as e:
        logger.info(f"Successfully caught expected ResponseError for non-existent model: {e.status_code} - {e.error}")
    except Exception as e:
        logger.error(f"Caught unexpected exception during non-existent model test: {e}", exc_info=True)
