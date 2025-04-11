import pytest
from unittest.mock import patch, MagicMock, call
from pathlib import Path

from src.harness import Harness
from src.ledger import Ledger

# Fixtures will be needed here to set up Harness and mock dependencies

@pytest.fixture
def temp_harness_work_dir(tmp_path):
    """Creates a temporary working directory for harness tests."""
    work_dir = tmp_path / "harness_loop_work_dir"
    work_dir.mkdir()
    # Create a dummy project dir inside for Aider/Pytest to run against
    (work_dir / "dummy_project").mkdir()
    # Create dummy config and goal files
    config_path = work_dir / "config.yaml"
    config_data = {
        "ollama_model": "mock-model",
        "project_dir": str(work_dir / "dummy_project"),
        "aider_command": "aider",
        "storage_type": "json", # Use JSON for easier inspection in tests if needed
        "enable_council": False, # Disable council for these core tests
        "enable_code_review": False,
    }
    import yaml
    with open(config_path, 'w') as f:
        yaml.dump(config_data, f)
        
    goal_path = work_dir / "goal.prompt"
    goal_path.write_text("Initial test goal.")
    
    return work_dir

@pytest.fixture
def harness_instance(temp_harness_work_dir):
    """Provides a Harness instance initialized in a temporary directory."""
    # Use reset_state=True to ensure clean state for each test
    harness = Harness(
        config_file=str(temp_harness_work_dir / "config.yaml"),
        work_dir=temp_harness_work_dir,
        max_retries=3, # Keep low for testing
        reset_state=True,
        enable_council=False # Explicitly disable council for these tests
    )
    # Ensure project dir exists within the harness config
    Path(harness.config["project_dir"]).mkdir(parents=True, exist_ok=True)
    return harness

# --- Test Implementations ---

@pytest.mark.loop
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_aider_returns_diff_output(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_instance):
    """Validate Aider returns non-empty code or patch diff during the loop."""
    # Mock Aider to return a specific diff
    mock_run_aider.return_value = ("```diff\n--- a/file.py\n+++ b/file.py\n@@ -1,1 +1,1 @@\n-hello\n+world\n```", None)
    # Mock pytest to pass
    mock_run_pytest.return_value = (True, "Pytest passed")
    # Mock LLM evaluation to return SUCCESS to stop the loop after one iteration
    mock_get_llm_response.return_value = "Verdict: SUCCESS\nSuggestions: "
    
    harness_instance.run("Test goal")
    
    # Assert run_aider was called
    mock_run_aider.assert_called_once()
    # Check the history passed to run_aider (optional, more detailed check)
    # args, kwargs = mock_run_aider.call_args
    # assert kwargs['history'] == [] # First call, history is empty before prompt
    
    # Check that the diff was recorded in the ledger (indirectly checks if it was processed)
    run_summary = harness_instance.ledger.get_run_summary(harness_instance.current_run_id)
    assert len(run_summary["iterations"]) == 1
    assert run_summary["iterations"][0]["aider_diff"] == "```diff\n--- a/file.py\n+++ b/file.py\n@@ -1,1 +1,1 @@\n-hello\n+world\n```"

@pytest.mark.loop
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_pytest_executes_after_diff(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_instance):
    """Ensure pytest runs against updated files after each patch."""
    # Mock Aider to return a diff
    mock_run_aider.return_value = ("fake diff", None)
    # Mock pytest to pass
    mock_run_pytest.return_value = (True, "Pytest passed")
    # Mock LLM evaluation to return SUCCESS
    mock_get_llm_response.return_value = "Verdict: SUCCESS\nSuggestions: "
    
    harness_instance.run("Test goal")
    
    # Assert run_pytest was called after run_aider
    mock_run_aider.assert_called_once()
    mock_run_pytest.assert_called_once()
    # We expect run_pytest to be called with the project directory from config
    mock_run_pytest.assert_called_once_with(harness_instance.config["project_dir"])

@pytest.mark.loop
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_local_llm_evaluates_result(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_instance):
    """Check that Ollama gives a response based on pytest output."""
    # Mock Aider
    mock_run_aider.return_value = ("fake diff", None)
    # Mock pytest
    mock_run_pytest.return_value = (False, "Pytest FAILED: Assertion Error")
    # Mock LLM evaluation to return RETRY (simulate evaluation based on failure)
    # We don't need to mock the *content* of the LLM response here, just that it's called
    # and the harness uses its *parsed* result (which we simulate below).
    # Let's make the mock return a RETRY verdict.
    mock_get_llm_response.return_value = "Verdict: RETRY\nSuggestions: Fix the assertion error."

    # Run the harness for one iteration (it should stop due to RETRY if max_retries=1, or continue)
    harness_instance.max_retries = 1 # Ensure it stops after the first RETRY
    harness_instance.run("Test goal")

    # Assert get_llm_response was called
    mock_get_llm_response.assert_called_once()
    
    # Check the arguments passed to the LLM evaluator
    args, kwargs = mock_get_llm_response.call_args
    evaluation_prompt = args[0] # The first positional argument is the prompt
    
    # Check if the prompt contains key elements from the iteration
    assert "Current Goal:\nTest goal" in evaluation_prompt # Expect "Current Goal:" now
    assert "Last Code Changes (diff):\n```diff\nfake diff\n```" in evaluation_prompt
    assert "Test Results:\nStatus: FAILED" in evaluation_prompt
    assert "Pytest FAILED: Assertion Error" in evaluation_prompt
    
    # Check the final state reflects the RETRY verdict (since max_retries = 1)
    assert not harness_instance.state["converged"]
    assert "Max retries reached" in harness_instance.state["last_error"]

@pytest.mark.loop
@patch('src.harness.run_aider')
@patch('src.harness.run_pytest')
@patch('src.harness.get_llm_response')
def test_loop_retries_if_not_converged(mock_get_llm_response, mock_run_pytest, mock_run_aider, harness_instance):
    """Harness must re-attempt improvement if Ollama says 'retry'."""
    # Set max_retries high enough to allow for a retry
    harness_instance.max_retries = 2
    
    # Mock Aider responses for two calls
    aider_responses = [
        ("diff attempt 1", None), # First attempt
        ("diff attempt 2", None)  # Second attempt (after retry)
    ]
    mock_run_aider.side_effect = aider_responses
    
    # Mock pytest responses
    pytest_responses = [
        (False, "Pytest FAILED on attempt 1"), # First attempt fails
        (True, "Pytest PASSED on attempt 2")   # Second attempt passes
    ]
    mock_run_pytest.side_effect = pytest_responses
    
    # Mock LLM evaluation responses
    llm_responses = [
        "Verdict: RETRY\nSuggestions: Fix the failure from attempt 1.", # First evaluation -> RETRY
        "Verdict: SUCCESS\nSuggestions: "                               # Second evaluation -> SUCCESS
    ]
    mock_get_llm_response.side_effect = llm_responses

    # Run the harness
    result = harness_instance.run("Test goal")

    # Assert Aider was called twice
    assert mock_run_aider.call_count == 2
    # Assert pytest was called twice
    assert mock_run_pytest.call_count == 2
    # Assert LLM evaluation was called twice
    assert mock_get_llm_response.call_count == 2
    
    # Check the prompts passed to Aider
    aider_calls = mock_run_aider.call_args_list
    assert aider_calls[0].kwargs['prompt'] == "Test goal" # First call uses initial goal
    assert "Fix the failure from attempt 1." in aider_calls[1].kwargs['prompt'] # Second call uses retry prompt
    
    # Check the final state
    assert result["converged"] is True
    assert result["iterations"] == 2
    assert result["final_status"] == "SUCCESS"
    
    # Check ledger reflects two iterations
    run_summary = harness_instance.ledger.get_run_summary(harness_instance.current_run_id)
    assert len(run_summary["iterations"]) == 2
    assert run_summary["iterations"][0]["llm_verdict"] == "RETRY"
    assert run_summary["iterations"][1]["llm_verdict"] == "SUCCESS"
