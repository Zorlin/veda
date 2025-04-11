import pytest
from unittest.mock import patch, MagicMock
from pathlib import Path

from src.harness import Harness

# --- Fixtures ---

@pytest.fixture
def temp_harness_work_dir(tmp_path):
    """Creates a temporary working directory for harness evaluation tests."""
    work_dir = tmp_path / "harness_eval_work_dir"
    work_dir.mkdir()
    # Create dummy config file
    config_path = work_dir / "config.yaml"
    config_data = {
        "ollama_model": "mock-eval-model",
        "project_dir": str(work_dir / "dummy_project"), # Needs to exist for Harness init
        "enable_council": False, # Ensure council is disabled
        "storage_type": "json", # Use JSON for simplicity in these tests
    }
    import yaml
    with open(config_path, 'w') as f:
        yaml.dump(config_data, f)
    # Create dummy project dir
    (work_dir / "dummy_project").mkdir()
    return work_dir

@pytest.fixture
def harness_eval_instance(temp_harness_work_dir):
    """Provides a Harness instance for evaluation tests (council disabled)."""
    harness = Harness(
        config_file=str(temp_harness_work_dir / "config.yaml"),
        work_dir=temp_harness_work_dir,
        reset_state=True,
        enable_council=False # Explicitly disable council
    )
    # Initialize state for evaluation context
    harness.state = {
        "current_iteration": 1,
        "prompt_history": [{"role": "user", "content": "Initial Goal"}],
        "converged": False,
        "last_error": None,
        "run_id": 1 # Assume a run is active
    }
    return harness

# --- Test Implementations ---

@pytest.mark.llm
@patch('src.harness.get_llm_response')
def test_llm_handles_successful_output(mock_get_llm_response, harness_eval_instance):
    """Ollama must correctly identify successful output from pytest logs."""
    # Mock LLM to return SUCCESS verdict
    mock_get_llm_response.return_value = "Verdict: SUCCESS\nSuggestions: "
    
    initial_goal = "Test Goal for Success"
    aider_diff = "Successful diff"
    pytest_output = "All tests passed!"
    pytest_passed = True
    
    verdict, suggestions = harness_eval_instance._evaluate_outcome(
        initial_goal, aider_diff, pytest_output, pytest_passed
    )
    
    # Assert LLM was called
    mock_get_llm_response.assert_called_once()
    # Assert the prompt passed to LLM contains relevant info
    call_args, _ = mock_get_llm_response.call_args
    prompt_text = call_args[0]
    assert initial_goal in prompt_text
    assert aider_diff in prompt_text
    assert pytest_output in prompt_text
    assert "Status: PASSED" in prompt_text
    
    # Assert the verdict and suggestions are correct
    assert verdict == "SUCCESS"
    assert suggestions == ""

@pytest.mark.llm
@patch('src.harness.get_llm_response')
def test_llm_handles_failed_output_and_suggests_retry(mock_get_llm_response, harness_eval_instance):
    """When given failed output, LLM must respond with a retry plan."""
    # Mock LLM to return RETRY verdict with suggestions
    expected_suggestions = "Fix the assertion error in test_example."
    mock_get_llm_response.return_value = f"Verdict: RETRY\nSuggestions: {expected_suggestions}"
    
    initial_goal = "Test Goal for Failure"
    aider_diff = "Diff that caused failure"
    pytest_output = "FAILED tests/test_example.py::test_example - AssertionError"
    pytest_passed = False
    
    verdict, suggestions = harness_eval_instance._evaluate_outcome(
        initial_goal, aider_diff, pytest_output, pytest_passed
    )
    
    # Assert LLM was called
    mock_get_llm_response.assert_called_once()
    # Assert the prompt passed to LLM contains relevant info
    call_args, _ = mock_get_llm_response.call_args
    prompt_text = call_args[0]
    assert initial_goal in prompt_text
    assert aider_diff in prompt_text
    assert pytest_output in prompt_text
    assert "Status: FAILED" in prompt_text
    
    # Assert the verdict and suggestions are correct
    assert verdict == "RETRY"
    assert suggestions == expected_suggestions

@pytest.mark.llm
@patch('src.harness.get_llm_response')
def test_llm_flags_invalid_or_unusable_output(mock_get_llm_response, harness_eval_instance):
    """If output is invalid Python or contradicts intent, LLM must block it (FAILURE)."""
    # Mock LLM to return FAILURE verdict
    mock_get_llm_response.return_value = "Verdict: FAILURE\nSuggestions: The generated code is syntactically incorrect and does not address the goal."
    
    initial_goal = "Generate a complex function"
    # Simulate Aider producing syntactically invalid code
    aider_diff = "```diff\n--- a/file.py\n+++ b/file.py\n@@ -1,1 +1,1 @@\ndef my_func(\n-pass\n+  print('hello world\n```"
    # Pytest might pass if the file isn't imported or run, or fail with SyntaxError
    pytest_output = "ERROR collecting tests/test_file.py - SyntaxError: unexpected EOF while parsing"
    pytest_passed = False # Pytest collection failed
    
    verdict, suggestions = harness_eval_instance._evaluate_outcome(
        initial_goal, aider_diff, pytest_output, pytest_passed
    )
    
    # Assert LLM was called
    mock_get_llm_response.assert_called_once()
    # Assert the prompt passed to LLM contains relevant info
    call_args, _ = mock_get_llm_response.call_args
    prompt_text = call_args[0]
    assert initial_goal in prompt_text
    assert aider_diff in prompt_text
    assert pytest_output in prompt_text
    assert "Status: FAILED" in prompt_text # Even if pytest fails collection, it's still a failure
    
    # Assert the verdict is FAILURE and suggestions are empty (as per format)
    assert verdict == "FAILURE"
    assert suggestions == "" # Suggestions should be empty for FAILURE verdict
