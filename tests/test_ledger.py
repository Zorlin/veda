import pytest
import tempfile
from pathlib import Path
import json
import sqlite3
import os

from src.ledger import Ledger

@pytest.fixture
def temp_work_dir():
    """Create a temporary working directory."""
    with tempfile.TemporaryDirectory() as tmpdirname:
        yield Path(tmpdirname)

@pytest.mark.ledger
def test_ledger_initialization_sqlite(temp_work_dir):
    """Test that SQLite ledger initializes correctly."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="sqlite"
    )
    
    # Check that database file was created
    db_path = temp_work_dir / "harness_ledger.db"
    assert db_path.exists()
    
    # Check that tables were created
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()
    
    # Get list of tables
    cursor.execute("SELECT name FROM sqlite_master WHERE type='table';")
    tables = [row[0] for row in cursor.fetchall()]
    
    assert "runs" in tables
    assert "iterations" in tables
    assert "messages" in tables
    assert "council_evaluations" in tables
    
    conn.close()

@pytest.mark.ledger
def test_ledger_initialization_json(temp_work_dir):
    """Test that JSON ledger initializes correctly."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="json"
    )
    
    # Check that JSON file was created
    json_path = temp_work_dir / "harness_state.json"
    assert json_path.exists()
    
    # Check that file contains expected structure
    with open(json_path, 'r') as f:
        state = json.load(f)
    
    assert "runs" in state
    assert "current_run" in state
    assert "metadata" in state

@pytest.mark.ledger
def test_start_run_sqlite(temp_work_dir):
    """Test starting a run with SQLite storage."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="sqlite"
    )
    
    run_id = ledger.start_run(
        initial_goal="Test goal",
        max_retries=5,
        config={"test_key": "test_value"}
    )
    
    # Check that run was created
    conn = sqlite3.connect(temp_work_dir / "harness_ledger.db")
    cursor = conn.cursor()
    cursor.execute("SELECT * FROM runs WHERE run_id = ?", (run_id,))
    run = cursor.fetchone()
    conn.close()
    
    assert run is not None
    assert run[3] == "Test goal"  # initial_goal
    assert run[4] == 5  # max_retries

@pytest.mark.ledger
def test_start_run_json(temp_work_dir):
    """Test starting a run with JSON storage."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="json"
    )
    
    run_id = ledger.start_run(
        initial_goal="Test goal",
        max_retries=5,
        config={"test_key": "test_value"}
    )
    
    # Check that run was created
    with open(temp_work_dir / "harness_state.json", 'r') as f:
        state = json.load(f)
    
    assert len(state["runs"]) == 1
    assert state["runs"][0]["run_id"] == run_id
    assert state["runs"][0]["initial_goal"] == "Test goal"
    assert state["runs"][0]["max_retries"] == 5
    assert state["current_run"] == run_id

@pytest.mark.ledger
def test_add_message_and_get_history(temp_work_dir):
    """Test adding messages and retrieving conversation history."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="json"  # Use JSON for easier testing
    )
    
    run_id = ledger.start_run("Test goal", 5, {})
    
    # Add messages
    ledger.add_message(run_id, None, "user", "Hello")
    ledger.add_message(run_id, None, "assistant", "Hi there")
    ledger.add_message(run_id, None, "user", "How are you?")
    
    # Get history
    history = ledger.get_conversation_history(run_id)
    
    assert len(history) == 3
    assert history[0]["role"] == "user"
    assert history[0]["content"] == "Hello"
    assert history[1]["role"] == "assistant"
    assert history[1]["content"] == "Hi there"
    assert history[2]["role"] == "user"
    assert history[2]["content"] == "How are you?"

@pytest.mark.ledger
def test_iteration_workflow(temp_work_dir):
    """Test the complete iteration workflow."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="json"  # Use JSON for easier testing
    )
    
    # Start run
    run_id = ledger.start_run("Test goal", 5, {})
    
    # Start iteration
    iteration_id = ledger.start_iteration(run_id, 1, "Test prompt")
    
    # Complete iteration
    ledger.complete_iteration(
        run_id,
        iteration_id,
        "test diff",
        "test output",
        True,
        "SUCCESS",
        ""
    )
    
    # Add council evaluation
    ledger.add_council_evaluation(
        iteration_id,
        "test_model",
        "theorist",
        "Good approach",
        0.9
    )
    
    # Get run summary
    summary = ledger.get_run_summary(run_id)
    
    assert summary["run_id"] == run_id
    assert summary["iteration_count"] == 1
    assert len(summary["iterations"]) == 1
    assert summary["iterations"][0]["iteration_id"] == iteration_id
    assert summary["iterations"][0]["pytest_passed"] == True
    assert summary["iterations"][0]["llm_verdict"] == "SUCCESS"
    
    # Check council evaluation
    with open(temp_work_dir / "harness_state.json", 'r') as f:
        state = json.load(f)
    
    assert "council_evaluations" in state["runs"][0]["iterations"][0]
    assert len(state["runs"][0]["iterations"][0]["council_evaluations"]) == 1
    assert state["runs"][0]["iterations"][0]["council_evaluations"][0]["role"] == "theorist"
    assert state["runs"][0]["iterations"][0]["council_evaluations"][0]["score"] == 0.9

@pytest.mark.ledger
def test_end_run(temp_work_dir):
    """Test ending a run."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="json"  # Use JSON for easier testing
    )
    
    run_id = ledger.start_run("Test goal", 5, {})
    ledger.end_run(run_id, True, "SUCCESS")
    
    # Check that run was updated
    with open(temp_work_dir / "harness_state.json", 'r') as f:
        state = json.load(f)
    
    assert state["runs"][0]["converged"] == True
    assert state["runs"][0]["final_status"] == "SUCCESS"
    assert state["runs"][0]["end_time"] is not None
    assert state["current_run"] is None
