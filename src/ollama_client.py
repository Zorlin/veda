import httpx
import json
import logging
import asyncio
from typing import Optional, Dict, Any

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class OllamaClient:
    """A client for interacting with the Ollama API."""

    def __init__(
        self,
        api_url: str,
        model: str,
        timeout: int = 300,
        options: Optional[Dict[str, Any]] = None,
    ):
        """Initializes the OllamaClient.

        Args:
            api_url: The base URL for the Ollama API (e.g., "http://localhost:11434/api/generate").
            model: The name of the Ollama model to use.
            timeout: Request timeout in seconds.
            options: Optional dictionary of Ollama parameters (e.g., temperature, top_p).
        """
        if not api_url:
            raise ValueError("Ollama API URL must be provided.")
        if not model:
            raise ValueError("Ollama model name must be provided.")

        self.api_url = api_url
        self.model = model
        self.timeout = timeout
        self.options = options if options else {}
        # Use synchronous client for worker threads
        self._client = httpx.Client(timeout=self.timeout)
        logger.info(f"OllamaClient initialized for model '{self.model}' at {self.api_url}")

    def generate(self, prompt: str) -> str:
        """Sends a prompt to the Ollama API and returns the generated response.

        Args:
            prompt: The input prompt for the model.

        Returns:
            The generated text response from the model, or an error message.
        """
        payload = {
            "model": self.model,
            "prompt": prompt,
            "stream": False, # Keep it simple for now, get full response
            "options": self.options,
        }
        logger.debug(f"Sending request to Ollama: {self.model}")
        try:
            # Synchronous call
            response = self._client.post(self.api_url, json=payload)
            response.raise_for_status()  # Raise an exception for bad status codes (4xx or 5xx)

            response_data = response.json()
            logger.debug(f"Received response from Ollama")

            if "response" in response_data:
                return response_data["response"].strip()
            else:
                logger.error(f"Unexpected response format from Ollama")
                return "[Error: Unexpected response format from Ollama]"

        except httpx.TimeoutException:
            logger.error(f"Request to Ollama timed out after {self.timeout} seconds.")
            return f"[Error: Request timed out after {self.timeout}s]"
        except httpx.RequestError as e:
            logger.error(f"Error connecting to Ollama API at {self.api_url}: {e}")
            return f"[Error: Could not connect to Ollama API: {e}]"
        except json.JSONDecodeError:
            logger.error(f"Error decoding JSON response from Ollama")
            return "[Error: Invalid JSON response from Ollama]"
        except Exception as e:
            logger.exception(f"An unexpected error occurred during Ollama request: {e}")
            return f"[Error: An unexpected error occurred: {e}]"

    async def generate_async(self, prompt: str) -> str:
        """Asynchronous version of generate that runs in a thread pool.
        
        This allows the synchronous generate method to be called from async code.
        """
        loop = asyncio.get_running_loop()
        return await loop.run_in_executor(None, self.generate, prompt)

    def close(self):
        """Close the client session."""
        if hasattr(self, '_client') and self._client:
            self._client.close()
            logger.debug("OllamaClient closed")
