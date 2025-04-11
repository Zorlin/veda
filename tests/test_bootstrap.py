import pytest
from pathlib import Path
import subprocess
import os
import requests # For Ollama check

# Assuming Harness class is importable
# from src.harness import Harness # We might need this later

# TODO: Determine the best way to access config/state for tests
# Maybe fixtures that set up a temporary harness instance?

@pytest.mark.bootstrap
def test_harness_initializes_config_directory(tmp_path):
    """Ensure the working directory, logs, and config files are initialized."""
    # We need to instantiate Harness or call a setup function
    # For now, let's just check if the directory can be created
    work_dir = tmp_path / "test_harness_work_dir"
    # Simulating what Harness.__init__ does regarding directory creation
    work_dir.mkdir(parents=True, exist_ok=True)
    assert work_dir.is_dir()
    # Placeholder for checking log dir/config file creation within work_dir
    # Example:
    # log_dir = work_dir / "logs"
    # log_dir.mkdir() # Simulate logger creation
    # assert log_dir.is_dir()
    # config_path = work_dir / "config.yaml" # Assuming default name
    # config_path.touch() # Simulate config loading/creation
    # assert config_path.is_file()
    pytest.skip("Test needs Harness instantiation or setup fixture.")


@pytest.mark.bootstrap
def test_ollama_model_is_accessible():
    """Validate Ollama can be called and returns basic output."""
    # Assuming default Ollama URL from harness defaults for now
    ollama_api_url = "http://localhost:11434/api/generate"
    ollama_model = "llama3" # Assuming default model

    try:
        response = requests.post(
            ollama_api_url,
            json={
                "model": ollama_model,
                "prompt": "Why is the sky blue?", # Simple test prompt
                "stream": False, # Get a single response object
                "options": {"num_predict": 10} # Limit output size
            },
            timeout=10 # Add a timeout
        )
        response.raise_for_status() # Raise exception for bad status codes (4xx or 5xx)
        data = response.json()
        assert "response" in data
        assert isinstance(data["response"], str)
        assert len(data["response"]) > 0
        print(f"\nOllama response snippet: {data['response'][:50]}...") # Print snippet for confirmation
    except requests.exceptions.ConnectionError:
        pytest.fail(f"Could not connect to Ollama at {ollama_api_url}. Is Ollama running?")
    except requests.exceptions.Timeout:
        pytest.fail(f"Request to Ollama timed out ({ollama_api_url}).")
    except requests.exceptions.RequestException as e:
        pytest.fail(f"Ollama request failed: {e}")
    except Exception as e:
        pytest.fail(f"An unexpected error occurred while testing Ollama: {e}")


@pytest.mark.bootstrap
def test_aider_starts_and_receives_prompt():
    """Ensure Aider subprocess can be called with a test prompt."""
    aider_command = "aider" # Assuming aider is in PATH (from harness defaults)
    test_prompt = "What is the current directory?"

    try:
        # Use --yes to auto-accept changes (though none should happen here)
        # Use a simple, non-modifying prompt
        # Capture output, set a timeout
        process = subprocess.run(
            [aider_command, "--yes", test_prompt],
            capture_output=True,
            text=True,
            timeout=30, # Adjust timeout as needed
            check=False # Don't fail on non-zero exit code initially
        )

        # Basic checks: Did it run without immediate error? Did it mention the prompt?
        # Aider's exit code might be non-zero if it expects more interaction or finds no git repo
        # So we primarily check stderr/stdout for signs of life and prompt processing.
        print(f"\nAider stdout:\n{process.stdout[-500:]}") # Show tail of stdout
        print(f"\nAider stderr:\n{process.stderr[-500:]}") # Show tail of stderr
        # A more robust check would involve specific output patterns,
        # but this confirms the subprocess runs.
        assert process.returncode is not None # Check if process terminated
        # Check if the prompt appears somewhere in the output (might be in logs/stderr)
        # This is a weak check, might need refinement based on aider's actual output
        assert test_prompt in process.stdout or test_prompt in process.stderr or "Processing message" in process.stderr

    except FileNotFoundError:
        pytest.fail(f"Aider command '{aider_command}' not found. Is aider installed and in PATH?")
    except subprocess.TimeoutExpired:
        pytest.fail("Aider subprocess timed out.")
    except Exception as e:
        pytest.fail(f"An unexpected error occurred while running aider: {e}")

# TODO: Add fixtures later to manage Harness instance, temp directories, etc.
