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
def test_loop_stops_on_converged_success(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_converge_instance):
    """Harness should stop looping after clear success verdict."""
    # Mock Aider to return a diff
    mock_run_aider.return_value = ("diff_success", None)
    # Mock pytest to pass
    mock_run_pytest.return_value = (True, "Pytest PASSED")
    # Mock LLM evaluation to return SUCCESS
    mock_get_llm_response.return_value = "Verdict: SUCCESS\nSuggestions: "
    
    result = harness_converge_instance.run("Test Goal for Success")
    
    # Assert the loop ran only once
    assert mock_run_aider.call_count == 1
    assert mock_run_pytest.call_count == 1
    assert mock_get_llm_response.call_count == 1
    
    # Assert final state is converged success
    assert result["converged"] is True
    assert result["final_status"] == "SUCCESS"
    assert result["iterations"] == 1 # Iteration count should be 1 (since it starts at 0)

@pytest.mark.convergence
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_loop_stops_on_max_retries(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_converge_instance):
    """If max retry count is reached, the loop should exit cleanly."""
    max_retries = 2
    harness_converge_instance.max_retries = max_retries
    
    # Mock Aider to return different diffs each time
    mock_run_aider.side_effect = [
        ("diff_retry_1", None),
        ("diff_retry_2", None),
        ("diff_retry_3", None), # Should not be called if max_retries=2
    ]
    # Mock pytest to always fail
    mock_run_pytest.return_value = (False, "Pytest FAILED")
    # Mock LLM evaluation to always return RETRY
    mock_get_llm_response.return_value = "Verdict: RETRY\nSuggestions: Keep trying"
    
    result = harness_converge_instance.run("Test Goal for Max Retries")
    
    # Assert the loop ran max_retries times
    assert mock_run_aider.call_count == max_retries
    assert mock_run_pytest.call_count == max_retries
    assert mock_get_llm_response.call_count == max_retries
    
    # Assert final state is not converged and indicates max retries
    assert result["converged"] is False
    assert "MAX_RETRIES_REACHED" in result["final_status"]
    assert "Max retries reached" in harness_converge_instance.state["last_error"]
    assert result["iterations"] == max_retries

@pytest.mark.convergence
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_loop_detects_stuck_cycle_and_aborts(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_converge_instance):
    """Loop must detect non-progressing diffs (repeated diffs) and exit."""
    max_retries = 5 # Set higher than the stuck cycle threshold
    harness_converge_instance.max_retries = max_retries
    stuck_cycle_threshold = 2 # As defined in harness.py
    
    # Mock Aider to return the *same* diff multiple times
    repeated_diff = "diff_stuck"
    mock_run_aider.side_effect = [
        ("diff_initial", None), # First iteration
        (repeated_diff, None),  # Second iteration
        (repeated_diff, None),  # Third iteration (should trigger stuck cycle)
        ("diff_after_stuck", None), # Should not be called
    ]
    # Mock pytest to fail (doesn't matter much for this test)
    mock_run_pytest.return_value = (False, "Pytest FAILED")
    # Mock LLM evaluation to always return RETRY (to keep the loop going)
    mock_get_llm_response.return_value = "Verdict: RETRY\nSuggestions: Try again"
    
    result = harness_converge_instance.run("Test Goal for Stuck Cycle")
    
    # Assert the loop stopped after detecting the stuck cycle
    # It runs iter 1 (diff_initial), iter 2 (repeated_diff), iter 3 (repeated_diff -> stuck)
    expected_aider_calls = stuck_cycle_threshold + 1
    expected_pytest_llm_calls = stuck_cycle_threshold # Pytest/LLM not called in the final stuck iteration
        
    assert mock_run_aider.call_count == expected_aider_calls
    # Pytest and LLM are called *before* the stuck check in the loop for the *next* iteration
    assert mock_run_pytest.call_count == expected_pytest_llm_calls
    assert mock_get_llm_response.call_count == expected_pytest_llm_calls
        
    # Assert final state is not converged and indicates stuck cycle
    assert result["converged"] is False
    assert "ERROR" in result["final_status"]
    assert "Stuck cycle detected" in result["final_status"]
    assert "Stuck cycle detected" in harness_converge_instance.state["last_error"]
    # Iteration count reflects when it stopped (after starting the iteration where stuck was detected)
    assert result["iterations"] == expected_aider_calls 
