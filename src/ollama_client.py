import httpx
import json
import logging
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
            api_url: The base URL for the Ollama API (e.g., "http://10.7.1.200:11434/api/generate").
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
        self._client = httpx.AsyncClient(timeout=self.timeout)
        logger.info(f"OllamaClient initialized for model '{self.model}' at {self.api_url}")

    async def generate(self, prompt: str) -> str:
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
        logger.debug(f"Sending request to Ollama: {payload}")
        try:
            response = await self._client.post(self.api_url, json=payload)
            response.raise_for_status()  # Raise an exception for bad status codes (4xx or 5xx)

            response_data = response.json()
            logger.debug(f"Received response from Ollama: {response_data}")

            if "response" in response_data:
                return response_data["response"].strip()
            else:
                logger.error(f"Unexpected response format from Ollama: {response_data}")
                return "[Error: Unexpected response format from Ollama]"

        except httpx.TimeoutException:
            logger.error(f"Request to Ollama timed out after {self.timeout} seconds.")
            return f"[Error: Request timed out after {self.timeout}s]"
        except httpx.RequestError as e:
            logger.error(f"Error connecting to Ollama API at {self.api_url}: {e}")
            return f"[Error: Could not connect to Ollama API: {e}]"
        except json.JSONDecodeError:
            logger.error(f"Error decoding JSON response from Ollama: {response.text}")
            return "[Error: Invalid JSON response from Ollama]"
        except Exception as e:
            logger.exception(f"An unexpected error occurred during Ollama request: {e}")
            return f"[Error: An unexpected error occurred: {e}]"

    async def close(self):
        """Closes the underlying HTTP client."""
        await self._client.aclose()
        logger.info("OllamaClient HTTP client closed.")

# Example usage (optional, for testing)
# if __name__ == "__main__":
#     import asyncio
#     import os
#     from config import load_config # Assuming config.py is accessible
#     from pathlib import Path

#     async def run_test():
#         try:
#             project_root = Path(__file__).parent.parent
#             cfg_path = project_root / "config.yaml"
#             cfg = load_config(cfg_path)

#             client = OllamaClient(
#                 api_url=cfg.get("ollama_api_url"),
#                 model=cfg.get("ollama_model"),
#                 timeout=cfg.get("ollama_request_timeout", 300),
#                 options=cfg.get("ollama_options")
#             )
#             prompt = "Why is the sky blue?"
#             print(f"Sending prompt: '{prompt}'")
#             response = await client.generate(prompt)
#             print(f"\nOllama Response:\n{response}")
#             await client.close()
#         except Exception as e:
#             print(f"Test failed: {e}")

#     asyncio.run(run_test())
