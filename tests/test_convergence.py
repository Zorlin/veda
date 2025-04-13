import pytest
from unittest.mock import patch, MagicMock, call
from pathlib import Path

from src.harness import Harness
from src.ledger import Ledger

# --- Fixtures ---

@pytest.fixture
def temp_harness_work_dir(tmp_path):
    """Creates a temporary working directory for convergence tests."""
    work_dir = tmp_path / "harness_convergence_work_dir"
    work_dir.mkdir()
    # Create dummy config file
    config_path = work_dir / "config.yaml"
    config_data = {
        "ollama_model": "mock-converge-model",
        "project_dir": str(work_dir / "dummy_project"),
        "enable_council": False, # Disable council for simplicity
        "storage_type": "sqlite", # Use SQLite for these tests
    }
    import yaml
    with open(config_path, 'w') as f:
        yaml.dump(config_data, f)
    # Create dummy project dir
    (work_dir / "dummy_project").mkdir()
    return work_dir

@pytest.fixture
def harness_converge_instance(temp_harness_work_dir):
    """Provides a Harness instance for convergence tests."""
    harness = Harness(
        config_file=str(temp_harness_work_dir / "config.yaml"),
        work_dir=temp_harness_work_dir,
        max_retries=3, # Keep low for testing
        reset_state=True, # Ensure clean state
        enable_council=False,
        storage_type="sqlite" # Explicitly use sqlite
    )
    return harness

# --- Test Implementations ---

@pytest.mark.convergence
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_loop_continues_on_success(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_converge_instance):
    """Harness should continue looping after SUCCESS verdict (never stop on success)."""
    # Mock Aider to return a diff
    mock_run_aider.return_value = ("diff_success", None)
    # Mock pytest to pass
    mock_run_pytest.return_value = (True, "Pytest PASSED")
    # Mock LLM evaluation to return SUCCESS
    mock_get_llm_response.return_value = "Verdict: SUCCESS\nSuggestions: "
    
    max_retries = 3
    harness_converge_instance.max_retries = max_retries
    result = harness_converge_instance.run("Test Goal for Success")
    
    # Assert the loop ran max_retries times (never stops on success)
    assert mock_run_aider.call_count == max_retries
    assert mock_run_pytest.call_count == max_retries
    assert mock_get_llm_response.call_count == max_retries
    
    # Assert final state is NOT converged (since loop never stops on success)
    assert result["converged"] is False
    assert "MAX_RETRIES_REACHED" in result["final_status"]
    assert result["iterations"] == max_retries

# (No change needed for test_loop_stops_on_max_retries: this test is still valid)

# (No change needed for test_loop_detects_stuck_cycle_and_aborts: this test is still valid)
