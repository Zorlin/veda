import pytest
from pathlib import Path
import subprocess
# import os # Unused import
import requests # For Ollama check - keeping for now, but will switch
import ollama # Use the official ollama library

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
@pytest.mark.long # Mark as a long-running test
def test_ollama_model_is_accessible():
    """Validate Ollama can be called and returns basic output."""
    # Assuming default Ollama URL from harness defaults for now
    # Using the ollama library instead of requests
    ollama_model = "gemma3:12b" # Use the intended default model

    try:
        # Check connection and model availability implicitly
        response = ollama.generate(
            model=ollama_model,
            prompt="Why is the sky blue?", # Simple test prompt
            stream=False, # Get a single response object
            options={"num_predict": 10} # Limit output size
        )
        # ollama library raises ollama.ResponseError on issues like 404 model not found
        assert "response" in response
        assert isinstance(response["response"], str)
        assert len(response["response"]) > 0
        assert response.get("done", False) is True # Check completion status
        print(f"\nOllama response snippet: {response['response'][:50]}...") # Print snippet for confirmation
    except ollama.ResponseError as e:
        # This catches errors like model not found (404)
        pytest.fail(f"Ollama API request failed: {e.status_code} - {e.error}")
    except ollama.ConnectionError as e:
        # Catch the specific connection error from the ollama library
        pytest.fail(f"Could not connect to Ollama: {e}. Is Ollama running and accessible?")
    except Exception as e:
        # Catch other potential errors (timeouts, unexpected issues)
        pytest.fail(f"An unexpected error occurred while testing Ollama: {e}")


@pytest.mark.bootstrap
@pytest.mark.long # Mark as a long-running test
def test_aider_starts_and_receives_prompt():
    """Ensure Aider subprocess can be called and returns basic output (version)."""
    aider_command = "aider" # Assuming aider is in PATH (from harness defaults)

    try:
        # Use --version command, which should be quick and not require LLM/git setup
        process = subprocess.run(
            [aider_command, "--version"],
            capture_output=True,
            text=True,
            timeout=60, # Increased timeout
            check=False, # Don't fail on non-zero exit code initially
            cwd=Path.cwd() # Run in the current working directory (project root)
        )

        # Basic checks: Did it run without immediate error? Did it mention the prompt?
        # Aider's exit code might be non-zero if it expects more interaction or finds no git repo
        # So we primarily check stderr/stdout for signs of life and prompt processing.
        print(f"\nAider stdout:\n{process.stdout[-500:]}") # Show tail of stdout
        print(f"\nAider stderr:\n{process.stderr[-500:]}") # Show tail of stderr
        # A more robust check would involve specific output patterns,
        # but this confirms the subprocess runs.
        assert process.returncode is not None # Check if process terminated
        # Check if the output contains typical version info (e.g., "aider", version number)
        # Aider version output might vary, adjust assertion as needed.
        # Example: "aider 0.81.2"
        assert process.returncode == 0 # --version should exit cleanly
        assert "aider" in process.stdout.lower()
        # Check for digits indicating a version number
        assert any(char.isdigit() for char in process.stdout)
        print(f"\nAider version output:\n{process.stdout}")

    except FileNotFoundError:
        pytest.fail(f"Aider command '{aider_command}' not found. Is aider installed and in PATH?")
    except subprocess.TimeoutExpired:
        pytest.fail("Aider subprocess timed out.")
    except Exception as e:
        pytest.fail(f"An unexpected error occurred while running aider: {e}")

# TODO: Add fixtures later to manage Harness instance, temp directories, etc.
