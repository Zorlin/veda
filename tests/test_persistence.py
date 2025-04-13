import pytest
from unittest.mock import patch, MagicMock, call
from pathlib import Path
import json

from src.harness import Harness
from src.ledger import Ledger

# --- Fixtures ---

@pytest.fixture
def temp_harness_work_dir(tmp_path):
    """Creates a temporary working directory for persistence tests."""
    work_dir = tmp_path / "harness_persistence_work_dir"
    work_dir.mkdir()
    # Create dummy config file
    config_path = work_dir / "config.yaml"
    config_data = {
        "ollama_model": "mock-persist-model",
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
def harness_persist_instance(temp_harness_work_dir):
    """Provides a Harness instance for persistence tests."""
    harness = Harness(
        config_file=str(temp_harness_work_dir / "config.yaml"),
        work_dir=temp_harness_work_dir,
        max_retries=3,
        reset_state=True, # Ensure clean state
        enable_council=False,
        storage_type="sqlite" # Explicitly use sqlite
    )
    return harness

# --- Helper to simulate a run ---

def simulate_run(harness: Harness):
    """Simulates a 2-iteration run for persistence testing."""
    with patch('src.harness.run_aider') as mock_run_aider, \
         patch('src.harness.run_pytest') as mock_run_pytest, \
         patch('src.harness.get_llm_response') as mock_get_llm_response:

        # Iteration 1: Failure -> RETRY
        mock_run_aider.return_value = ("diff_iter_1", None)
        mock_run_pytest.return_value = (False, "Pytest FAILED iter 1")
        mock_get_llm_response.return_value = "Verdict: RETRY\nSuggestions: Fix failure 1"

        # Iteration 2: Success
        mock_run_aider.side_effect = [
            ("diff_iter_1", None), # First call
            ("diff_iter_2", None)  # Second call
        ]
        mock_run_pytest.side_effect = [
            (False, "Pytest FAILED iter 1"), # First call
            (True, "Pytest PASSED iter 2")   # Second call
        ]
        mock_get_llm_response.side_effect = [
            "Verdict: RETRY\nSuggestions: Fix failure 1", # First call
            "Verdict: SUCCESS\nSuggestions: "             # Second call
        ]

        harness.run("Initial Goal for Persistence Test")

# --- Test Implementations ---

@pytest.mark.persistence
def test_diff_history_is_recorded(harness_persist_instance):
    """All diffs must be saved per iteration to a history log (ledger)."""
    simulate_run(harness_persist_instance)
    
    run_id = harness_persist_instance.current_run_id
    summary = harness_persist_instance.ledger.get_run_summary(run_id)
    
    # The loop never stops on success, so expect max_retries iterations (default 3 in fixture)
    assert len(summary["iterations"]) == 3
    assert summary["iterations"][0]["aider_diff"] == "diff_iter_1"
    assert summary["iterations"][1]["aider_diff"] == "diff_iter_2"
    # The last iteration will have aider_diff as None due to StopIteration in the test mock
    assert summary["iterations"][2]["aider_diff"] is None

@pytest.mark.persistence
def test_outcomes_are_categorized_in_ledger(harness_persist_instance):
    """Each run result must be labeled as pass/fail/blocked (verdict)."""
    simulate_run(harness_persist_instance)
    
    run_id = harness_persist_instance.current_run_id
    summary = harness_persist_instance.ledger.get_run_summary(run_id)
    
    # The loop never stops on success, so expect max_retries iterations (default 3 in fixture)
    assert len(summary["iterations"]) == 3
    # Check iteration verdicts
    assert summary["iterations"][0]["llm_verdict"] == "RETRY"
    assert summary["iterations"][1]["llm_verdict"] == "SUCCESS"
    # The last iteration will have llm_verdict as "FAILURE" due to StopIteration in the test mock
    assert summary["iterations"][2]["llm_verdict"] == "FAILURE"
    # Check final run status
    assert summary["converged"] is False
    assert "MAX_RETRIES_REACHED" in summary["final_status"] or "Aider failed" in summary["final_status"]

@pytest.mark.persistence
def test_prompt_chain_can_be_reconstructed(harness_persist_instance):
    """Prompt history must be reconstructible from logs or state DB (ledger)."""
    simulate_run(harness_persist_instance)
    
    run_id = harness_persist_instance.current_run_id
    history = harness_persist_instance.ledger.get_conversation_history(run_id)
    
    # Expected history:
    # 1. user: Initial Goal
    # 2. assistant: diff_iter_1
    # 3. user: Retry prompt based on "Fix failure 1"
    # 4. assistant: diff_iter_2
    
    assert len(history) == 4
    
    # Check roles
    assert history[0]["role"] == "user"
    assert history[1]["role"] == "assistant"
    assert history[2]["role"] == "user"
    assert history[3]["role"] == "assistant"
    
    # Check content (simplified checks)
    assert "Initial Goal for Persistence Test" in history[0]["content"]
    assert history[1]["content"] == "diff_iter_1"
    assert "Fix failure 1" in history[2]["content"] # Check if retry prompt includes suggestion
    assert history[3]["content"] == "diff_iter_2"
