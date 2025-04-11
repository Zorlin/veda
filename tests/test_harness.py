import json
import logging
import os
import shutil
from pathlib import Path
from unittest.mock import patch, MagicMock, mock_open

import pytest
import yaml

from src.harness import Harness

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
        "project_dir": resolved_project_dir,
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
    # The final work_dir should be project_dir / work_dir.name
    # expected_work_dir calculated above for cleanup
    assert harness.work_dir == expected_work_dir.resolve()

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
    # Check work_dir resolution (relative to loaded project_dir)
    expected_work_dir = expected_project_dir / temp_work_dir.name
    assert harness.work_dir == expected_work_dir.resolve()


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
    # Work dir should be resolved relative to this absolute path
    expected_work_dir = abs_project_path / temp_work_dir.name
    assert harness.work_dir == expected_work_dir.resolve()


# --- Test _initialize_state Method ---

def test_initialize_state_fresh_start(temp_work_dir, default_config): # Add default_config fixture
    """Test initializing state when no state file exists."""
    # Calculate expected resolved path and clean up any old state file there
    expected_work_dir = Path(default_config["project_dir"]) / temp_work_dir.name
    resolved_state_file = expected_work_dir.resolve() / "harness_state.json"
    resolved_state_file.unlink(missing_ok=True)

    # Initialize Harness - this will create the state file if it doesn't exist
    # Use default config file name, triggering resolution
    harness = Harness(work_dir=temp_work_dir, reset_state=False)

    # Assert that the state was initialized freshly, not that the file doesn't exist
    assert harness.state["current_iteration"] == 0
    assert harness.state["prompt_history"] == []
    assert not harness.state["converged"]
    # State file is only created on _save_state, not during init, so we don't assert its existence here.

def test_initialize_state_reset_flag(temp_work_dir, sample_state_path):
    """Test initializing state with reset_state=True ignores existing file."""
    # sample_state_path fixture creates the state file in temp_work_dir
    harness = Harness(work_dir=temp_work_dir, reset_state=True)
    assert harness.state["current_iteration"] == 0
    assert harness.state["prompt_history"] == []
    assert not harness.state["converged"]

def test_initialize_state_load_valid(temp_work_dir, sample_state_path):
    """Test loading a valid state file."""
    # Pass config_file=None to prevent work_dir resolution based on default config
    harness = Harness(config_file=None, work_dir=temp_work_dir, reset_state=False)
    assert harness.state["current_iteration"] == 2
    assert len(harness.state["prompt_history"]) == 3
    assert harness.state["prompt_history"][0]["role"] == "user"
    assert not harness.state["converged"]

def test_initialize_state_invalid_json(temp_work_dir, caplog):
    """Test initializing state when state file contains invalid JSON."""
    state_file = temp_work_dir / "harness_state.json"
    with open(state_file, "w") as f:
        f.write("{invalid_json,")
    # Pass config_file=None and set level for WARNING
    caplog.set_level(logging.WARNING)
    harness = Harness(config_file=None, work_dir=temp_work_dir, reset_state=False)

    assert f"Could not load or parse state file {state_file}" in caplog.text
    assert "Initializing fresh state." in caplog.text # Match exact log message
    assert harness.state["current_iteration"] == 0 # Should reset to default

def test_initialize_state_invalid_format(temp_work_dir, caplog):
    """Test initializing state when state file has incorrect structure."""
    state_file = temp_work_dir / "harness_state.json"
    invalid_state = {"iterations": 1, "history": []} # Missing keys
    with open(state_file, "w") as f:
        json.dump(invalid_state, f)
    # Pass config_file=None and set level for WARNING
    caplog.set_level(logging.WARNING)
    harness = Harness(config_file=None, work_dir=temp_work_dir, reset_state=False)

    assert f"State file {state_file} has invalid format. Initializing fresh state." in caplog.text # Match exact log message
    assert harness.state["current_iteration"] == 0 # Should reset to default

def test_initialize_state_invalid_history_type(temp_work_dir, caplog):
    """Test initializing state when prompt_history is not a list."""
    state_file = temp_work_dir / "harness_state.json"
    invalid_state = {
        "current_iteration": 1,
        "prompt_history": "not a list", # Invalid type
        "converged": False,
        "last_error": None,
    }
    with open(state_file, "w") as f:
        json.dump(invalid_state, f)
    # Pass config_file=None and set level for WARNING
    caplog.set_level(logging.WARNING)
    harness = Harness(config_file=None, work_dir=temp_work_dir, reset_state=False)

    assert "Loaded state has invalid 'prompt_history'. Resetting history." in caplog.text
    assert harness.state["current_iteration"] == 1 # Other fields loaded
    assert harness.state["prompt_history"] == [] # History reset

def test_initialize_state_io_error(temp_work_dir, caplog):
    """Test initializing state handles IOError during file read."""
    state_file = temp_work_dir / "harness_state.json"
    # Mock open to raise IOError when reading state file
    # Need to be careful mocking open as it's used for config too.
    # We'll initialize first, then mock for the state read part.
    harness = Harness(work_dir=temp_work_dir, reset_state=False) # Initialize first

    # Mock open specifically for the state file read attempt inside _initialize_state
    # This is tricky because _initialize_state is called by __init__.
    # A better approach might be to test _initialize_state directly.

    # Let's test _initialize_state directly
    # Pass config_file=None to prevent work_dir resolution issues
    harness_instance = Harness(config_file=None, work_dir=temp_work_dir, reset_state=True) # Get an instance with fresh state
    state_file_path = harness_instance.work_dir / "harness_state.json"
    # Ensure the state file exists so the open attempt happens
    state_file_path.touch()

    # Set log level for WARNING
    caplog.set_level(logging.WARNING)

    # Mock open to raise IOError specifically when opening the state file path
    original_open = open
    def mock_open_side_effect(file, mode='r', *args, **kwargs):
        if Path(file) == state_file_path and 'r' in mode:
            raise IOError("Cannot read state")
        # Fallback to original open for other files/modes if necessary
        # Be cautious with this fallback in complex tests
        return original_open(file, mode=mode, *args, **kwargs)

    with patch("builtins.open", side_effect=mock_open_side_effect):
        # Call _initialize_state directly
        state = harness_instance._initialize_state(reset_state=False)

    # Assert based on the actual path used
    assert f"Could not load or parse state file {state_file_path}: Cannot read state" in caplog.text
    assert "Initializing fresh state." in caplog.text # Match exact log message
    assert state["current_iteration"] == 0 # Should return default state

# --- Test _save_state Method ---

def test_save_state_creates_file(temp_work_dir, default_config): # Add default_config fixture
    """Test that _save_state creates the state file correctly."""
     # Calculate expected resolved path and clean up any old state file there
    expected_work_dir = Path(default_config["project_dir"]) / temp_work_dir.name
    resolved_state_file = expected_work_dir.resolve() / "harness_state.json"
    resolved_state_file.unlink(missing_ok=True)

    # Initialize Harness - this might create the state file during init
    harness = Harness(work_dir=temp_work_dir)
    state_file = harness.work_dir / "harness_state.json" # This is the resolved path

    # Modify state slightly (ensure it's different from default init state)
    harness.state["current_iteration"] = 1
    harness.state["prompt_history"].append({"role": "user", "content": "test"})

    harness._save_state()

    assert state_file.exists()
    with open(state_file, 'r') as f:
        saved_state = json.load(f)

    assert saved_state["current_iteration"] == 1
    assert len(saved_state["prompt_history"]) == 1
    assert saved_state["prompt_history"][0]["content"] == "test"

def test_save_state_creates_directory(tmp_path):
    """Test that _save_state creates the work directory if it doesn't exist."""
    non_existent_dir = tmp_path / "new_work_dir"
    assert not non_existent_dir.exists()

    # Pass config_file=None to prevent work_dir resolution
    harness = Harness(config_file=None, work_dir=non_existent_dir)
    harness._save_state()

    state_file = non_existent_dir / "harness_state.json"
    assert non_existent_dir.exists()
    assert state_file.exists()

def test_save_state_io_error(temp_work_dir, caplog):
    """Test _save_state handles IOError during file write."""
    # Pass config_file=None to prevent work_dir resolution
    harness = Harness(config_file=None, work_dir=temp_work_dir)
    state_file = harness.work_dir / "harness_state.json"
    # Ensure directory exists, as _save_state expects
    harness.work_dir.mkdir(parents=True, exist_ok=True)
    # Ensure file does *not* exist before the save attempt
    state_file.unlink(missing_ok=True)

    # Mock json.dump to raise IOError
    with patch("json.dump", side_effect=IOError("Disk full")) as mock_dump:
        harness._save_state()

    assert f"Could not write state file {state_file}: Disk full" in caplog.text
    # Check that json.dump was called with the harness state and a file handle
    mock_dump.assert_called_once()
    # Check the first argument passed to json.dump was the state
    assert mock_dump.call_args[0][0] == harness.state
    # Assert the file wasn't created or is empty due to the error
    assert not state_file.exists() or state_file.stat().st_size == 0

# TODO: Add tests for _create_evaluation_prompt, _create_retry_prompt, _evaluate_outcome (mocking LLM), and run (mocking subprocesses and LLM)
