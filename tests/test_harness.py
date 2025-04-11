import json
import logging
import os
import shutil
from pathlib import Path
from unittest.mock import patch, MagicMock, mock_open

import pytest
import yaml

from src.harness import Harness
from src.ledger import Ledger

# --- Fixtures ---

@pytest.fixture
def temp_work_dir(tmp_path):
    """Creates a temporary working directory for harness tests."""
    work_dir = tmp_path / "test_harness_work_dir"
    work_dir.mkdir()
    # Create a dummy project dir inside for relative path testing
    (work_dir.parent / "dummy_project").mkdir()
    yield work_dir
    # Clean up if needed, though tmp_path usually handles it
    # shutil.rmtree(work_dir, ignore_errors=True)
    # shutil.rmtree(work_dir.parent / "dummy_project", ignore_errors=True)


@pytest.fixture
def default_config():
    """Returns the default configuration dictionary."""
    # Resolve project_dir relative to the actual project root where pytest runs
    project_root = Path(__file__).parent.parent
    resolved_project_dir = str((project_root / ".").resolve())
    return {
        "ollama_model": "gemma3:12b",
        "ollama_api_url": "http://localhost:11434/api/generate",
        "aider_command": "aider",
        "aider_test_command": "pytest -v", # Added default
        "project_dir": resolved_project_dir,
        "ollama_request_timeout": 300, # Added default
        # UI Config Defaults
        "enable_ui": False,
        "websocket_host": "localhost",
        "websocket_port": 8765,
    }

@pytest.fixture
def sample_config_path(temp_work_dir):
    """Creates a sample config.yaml file."""
    config_path = temp_work_dir / "config.yaml"
    config_data = {
        "ollama_model": "test-model:latest",
        "project_dir": "dummy_project", # Relative path
        "extra_param": "value",
    }
    with open(config_path, 'w') as f:
        yaml.dump(config_data, f)
    return config_path

@pytest.fixture
def sample_state_path(temp_work_dir):
    """Creates a sample harness_state.json file."""
    state_path = temp_work_dir / "harness_state.json"
    state_data = {
        "current_iteration": 2,
        "prompt_history": [
            {"role": "user", "content": "Initial prompt"},
            {"role": "assistant", "content": "diff1"},
            {"role": "user", "content": "Retry prompt"},
        ],
        "converged": False,
        "last_error": None,
    }
    with open(state_path, 'w') as f:
        json.dump(state_data, f, indent=4)
    return state_path

# --- Test Harness Initialization and Config Loading ---

def test_harness_init_defaults(temp_work_dir, default_config):
    """Test Harness initialization with default parameters."""
    # Ensure config file doesn't exist initially
    config_file = temp_work_dir / "config.yaml"
    if config_file.exists():
        config_file.unlink()

    # Calculate expected resolved path and clean up any old state file there
    expected_work_dir = Path(default_config["project_dir"]) / temp_work_dir.name
    resolved_state_file = expected_work_dir.resolve() / "harness_state.json"
    resolved_state_file.unlink(missing_ok=True)

    harness = Harness(config_file=str(config_file), work_dir=temp_work_dir)

    assert harness.max_retries == 5
    # Check if default config is loaded (project_dir needs careful check)
    assert harness.config["ollama_model"] == default_config["ollama_model"]
    assert harness.config["aider_command"] == default_config["aider_command"]
    # The final work_dir should be the resolved path passed to the constructor
    assert harness.work_dir == temp_work_dir.resolve()

    # Check default state initialization (should be fresh as we cleaned the state file)
    assert harness.state["current_iteration"] == 0
    assert harness.state["prompt_history"] == []
    assert not harness.state["converged"]
    assert harness.state["last_error"] is None

def test_harness_init_with_config_file(temp_work_dir, sample_config_path, default_config):
    """Test Harness initialization loading from a config file."""
    harness = Harness(config_file=str(sample_config_path), work_dir=temp_work_dir)

    # Check config loaded from file overrides defaults
    assert harness.config["ollama_model"] == "test-model:latest"
    assert harness.config["extra_param"] == "value"
    # Check default values are still present if not overridden
    assert harness.config["aider_command"] == default_config["aider_command"]
    # Check project_dir resolution (relative to project root)
    project_root = Path(__file__).parent.parent
    expected_project_dir = (project_root / "dummy_project").resolve()
    assert harness.config["project_dir"] == str(expected_project_dir)
    # Check work_dir resolution (should be resolved relative to CWD, not project_dir)
    assert harness.work_dir == temp_work_dir.resolve()


def test_harness_init_override_model(temp_work_dir, sample_config_path):
    """Test overriding ollama_model via __init__ parameter."""
    harness = Harness(
        config_file=str(sample_config_path),
        work_dir=temp_work_dir,
        ollama_model="override-model:v1"
    )
    assert harness.config["ollama_model"] == "override-model:v1"

def test_harness_init_max_retries(temp_work_dir):
    """Test setting max_retries during initialization."""
    harness = Harness(work_dir=temp_work_dir, max_retries=10)
    assert harness.max_retries == 10

# --- Test _load_config Method ---

def test_load_config_file_not_found(temp_work_dir, default_config, caplog):
    """Test _load_config when the config file does not exist."""
    config_path = temp_work_dir / "nonexistent_config.yaml"
    harness = Harness(config_file=str(config_path), work_dir=temp_work_dir) # Instantiation calls _load_config

    # Check that the specific warning message with the full path is present
    assert f"Config file {config_path} not found. Using default configuration." in caplog.text
    assert harness.config == default_config # Should load defaults

def test_load_config_empty_file(temp_work_dir, default_config, caplog):
    """Test _load_config with an empty config file."""
    caplog.set_level(logging.INFO) # Ensure INFO messages are captured
    config_path = temp_work_dir / "empty_config.yaml"
    config_path.touch()
    harness = Harness(config_file=str(config_path), work_dir=temp_work_dir)

    assert f"Config file {config_path} is empty. Using defaults." in caplog.text
    assert harness.config == default_config

def test_load_config_invalid_yaml(temp_work_dir, default_config, caplog):
    """Test _load_config with a file containing invalid YAML."""
    config_path = temp_work_dir / "invalid_config.yaml"
    with open(config_path, "w") as f:
        f.write("ollama_model: test\n: invalid_yaml") # Invalid YAML syntax
    harness = Harness(config_file=str(config_path), work_dir=temp_work_dir)

    assert f"Error parsing config file {config_path}" in caplog.text
    assert harness.config == default_config

def test_load_config_not_a_dict(temp_work_dir, default_config, caplog):
    """Test _load_config when the YAML file doesn't contain a dictionary."""
    config_path = temp_work_dir / "list_config.yaml"
    with open(config_path, "w") as f:
        yaml.dump(["item1", "item2"], f) # Dump a list, not a dict
    harness = Harness(config_file=str(config_path), work_dir=temp_work_dir)

    assert f"Config file {config_path} does not contain a valid dictionary" in caplog.text
    assert harness.config == default_config

def test_load_config_io_error(temp_work_dir, default_config, caplog):
    """Test _load_config handles IOError during file read."""
    config_path = temp_work_dir / "unreadable_config.yaml"
    # Create the file so Path.is_file() passes
    config_path.touch()

    # Mock yaml.safe_load to raise IOError when called by _load_config
    with patch("yaml.safe_load", side_effect=IOError("Permission denied")):
        # Mock Path.is_file to ensure the code attempts to open the file
        with patch("pathlib.Path.is_file", return_value=True):
             harness = Harness(config_file=str(config_path), work_dir=temp_work_dir)

    # Check for the specific error log message
    assert f"Error reading config file {config_path}: Permission denied" in caplog.text
    assert harness.config == default_config # Should fall back to defaults

def test_load_config_project_dir_absolute(temp_work_dir, default_config):
    """Test _load_config correctly handles absolute project_dir."""
    abs_project_path = (temp_work_dir / "absolute_project").resolve()
    abs_project_path.mkdir()
    config_path = temp_work_dir / "abs_config.yaml"
    config_data = {"project_dir": str(abs_project_path)}
    with open(config_path, 'w') as f:
        yaml.dump(config_data, f)

    harness = Harness(config_file=str(config_path), work_dir=temp_work_dir)
    assert harness.config["project_dir"] == str(abs_project_path)
    # Work dir should be resolved relative to CWD, not project_dir
    assert harness.work_dir == temp_work_dir.resolve()

# --- Code Review Test ---

@patch('src.harness.get_llm_response')
def test_run_code_review_generates_review(mock_get_llm, temp_work_dir):
    """Test that _run_code_review calls the LLM and returns a formatted review."""
    # Setup Harness instance
    harness = Harness(work_dir=temp_work_dir, enable_code_review=True, storage_type="json")
    harness.current_run_id = 1 # Simulate an active run

    # Mock LLM response
    mock_review_content = "This code looks good overall.\n\n**Improvements:**\n- Add more comments."
    mock_get_llm.return_value = mock_review_content

    # Input data for the review
    initial_goal = "Implement feature X"
    aider_diff = "```diff\n+ new code\n```"
    pytest_output = "All tests passed."
 
    # Call the method
    review_result = harness.run_code_review(initial_goal, aider_diff, pytest_output)
 
    # Assert LLM was called once
    mock_get_llm.assert_called_once()
    call_args, call_kwargs = mock_get_llm.call_args
    
    # Assert prompt contains key elements
    review_prompt_arg = call_args[0]
    assert initial_goal in review_prompt_arg
    assert aider_diff in review_prompt_arg
    assert pytest_output in review_prompt_arg
    assert "Act as a senior code reviewer" in review_prompt_arg
    
    # Assert system prompt was passed
    assert "expert code reviewer" in call_kwargs.get("system_prompt", "")

    # Assert the returned result includes the header and the LLM content
    assert "# Code Review" in review_result
    assert f"**Run ID:** {harness.current_run_id}" in review_result
    assert "**Reviewer:** AI Code Reviewer" in review_result
    assert mock_review_content in review_result


# --- State Initialization Tests ---

@pytest.fixture
def resumable_ledger(temp_work_dir):
    """Creates a Ledger with an in-progress run for resume testing."""
    ledger = Ledger(work_dir=temp_work_dir, storage_type="json") # Use JSON for easier inspection if needed
    run_id = ledger.start_run("Initial Goal", 5, {"test_config": True})
    iter1_id = ledger.start_iteration(run_id, 1, "Prompt 1")
    ledger.add_message(run_id, iter1_id, "user", "Prompt 1")
    ledger.add_message(run_id, iter1_id, "assistant", "Diff 1")
    ledger.complete_iteration(run_id, iter1_id, "Diff 1", "Pytest Output 1", True, "RETRY", "Suggestion 1")
    # Start iteration 2 but don't complete it
    iter2_id = ledger.start_iteration(run_id, 2, "Prompt 2 (Retry)")
    ledger.add_message(run_id, iter2_id, "user", "Prompt 2 (Retry)")
    # The run is NOT ended
    return ledger, run_id

def test_initialize_state_fresh_start(temp_work_dir):
    """Test initializing state with no existing ledger state."""
    # Ensure ledger file doesn't exist (Ledger handles this)
    harness = Harness(config_file=None, work_dir=temp_work_dir, reset_state=False, storage_type="json")
    
    assert harness.state["current_iteration"] == 0
    assert harness.state["prompt_history"] == []
    assert not harness.state["converged"]
    assert harness.state["last_error"] is None
    assert harness.state["run_id"] is None

def test_initialize_state_reset_flag(resumable_ledger):
    """Test initializing state with reset_state=True ignores existing ledger state."""
    ledger, run_id = resumable_ledger
    work_dir = ledger.work_dir
    
    harness = Harness(config_file=None, work_dir=work_dir, reset_state=True, storage_type="json")
    
    assert harness.state["current_iteration"] == 0
    assert harness.state["prompt_history"] == []
    assert not harness.state["converged"]
    assert harness.state["last_error"] is None
    assert harness.state["run_id"] is None # Should ignore existing run_id

def test_initialize_state_load_valid_resumes_run(resumable_ledger):
    """Test initializing state loads and resumes an in-progress run from the ledger."""
    ledger, expected_run_id = resumable_ledger
    work_dir = ledger.work_dir
    
    # Initialize Harness without resetting state
    harness = Harness(config_file=None, work_dir=work_dir, reset_state=False, storage_type="json")
    
    # Check if the state reflects the resumed run
    assert harness.state["run_id"] == expected_run_id
    assert harness.state["current_iteration"] == 2 # Ledger reports 2 iterations started
    assert not harness.state["converged"] # Run wasn't finished
    assert harness.state["last_error"] is None # Run wasn't finished with an error
    
    # Check if history was loaded correctly
    assert len(harness.state["prompt_history"]) == 3 # user1, assistant1, user2
    assert harness.state["prompt_history"][0]["role"] == "user"
    assert harness.state["prompt_history"][0]["content"] == "Prompt 1"
    assert harness.state["prompt_history"][1]["role"] == "assistant"
    assert harness.state["prompt_history"][1]["content"] == "Diff 1"
    assert harness.state["prompt_history"][2]["role"] == "user"
    assert harness.state["prompt_history"][2]["content"] == "Prompt 2 (Retry)"

# Removed tests:
# - test_initialize_state_invalid_json
# - test_initialize_state_invalid_format
# - test_initialize_state_invalid_history_type
# - test_initialize_state_io_error
# These tests checked low-level JSON parsing/IO errors that are now the responsibility
# of the Ledger class and should be tested in test_ledger.py.

# Removed tests:
# - test_save_state_creates_file
# - test_save_state_creates_directory
# - test_save_state_io_error
# The Harness._save_state method was removed as state saving is now implicitly
# handled by the Ledger throughout the run and via ledger.end_run().

# TODO: Add tests for _create_evaluation_prompt, _create_retry_prompt, _evaluate_outcome (mocking LLM), and run (mocking subprocesses and LLM)
