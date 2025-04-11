import pytest
from pathlib import Path
import tempfile
import json
import os

from src.vesper_mind import VesperMind
from src.ledger import Ledger

@pytest.fixture
def temp_work_dir():
    """Create a temporary working directory."""
    with tempfile.TemporaryDirectory() as tmpdirname:
        yield Path(tmpdirname)

@pytest.fixture
def sample_ledger(temp_work_dir):
    """Create a sample ledger for testing."""
    ledger = Ledger(
        work_dir=temp_work_dir,
        storage_type="json"  # Use JSON for easier testing
    )
    return ledger

@pytest.fixture
def sample_config():
    """Create a sample configuration for testing."""
    return {
        "ollama_model": "gemma3:12b",
        "theorist_model": "gemma3:12b",  # Use the same model for all roles in testing
        "architect_model": "gemma3:12b",
        "skeptic_model": "gemma3:12b",
        "historian_model": "gemma3:12b",
        "coordinator_model": "gemma3:12b"
    }

@pytest.mark.vesper
def test_vesper_mind_initialization(temp_work_dir, sample_ledger, sample_config):
    """Test that VesperMind initializes correctly."""
    council = VesperMind(
        config=sample_config,
        ledger=sample_ledger,
        work_dir=temp_work_dir
    )
    
    # Check that council members are defined
    assert "theorist" in council.open_source_council
    assert "architect" in council.open_source_council
    assert "skeptic" in council.open_source_council
    assert "historian" in council.open_source_council
    assert "coordinator" in council.open_source_council
    
    assert "arbiter" in council.closed_source_council
    assert "canonizer" in council.closed_source_council
    assert "redactor" in council.closed_source_council

@pytest.mark.vesper
def test_synthesize_open_source_evaluations(temp_work_dir, sample_ledger, sample_config):
    """Test that open source evaluations are synthesized correctly."""
    council = VesperMind(
        config=sample_config,
        ledger=sample_ledger,
        work_dir=temp_work_dir
    )
    
    # Sample evaluations
    evaluations = {
        "theorist": {
            "evaluation": "Good theoretical approach",
            "score": 0.8,
            "concerns": ["Might not scale well"],
            "recommendations": ["Consider alternative algorithm"]
        },
        "architect": {
            "evaluation": "Implementation is solid",
            "score": 0.9,
            "concerns": [],
            "recommendations": ["Refactor for better readability"]
        }
    }
    
    summary = council._synthesize_open_source_evaluations(evaluations)
    
    # Check that summary contains expected content
    assert "Theorist Evaluation" in summary
    assert "Architect Evaluation" in summary
    assert "Good theoretical approach" in summary
    assert "Implementation is solid" in summary
    assert "Might not scale well" in summary
    assert "Refactor for better readability" in summary

@pytest.mark.vesper
def test_determine_final_verdict(temp_work_dir, sample_ledger, sample_config):
    """Test that final verdict is determined correctly."""
    council = VesperMind(
        config=sample_config,
        ledger=sample_ledger,
        work_dir=temp_work_dir
    )
    
    # Case 1: Arbiter says SUCCESS, Canonizer agrees
    council_results = {
        "open_source": {},
        "closed_source": {
            "arbiter": {
                "verdict": "SUCCESS",
                "suggestions": ""
            },
            "canonizer": {
                "verdict": "SUCCESS"
            },
            "redactor": {
                "evaluation": "All good"
            }
        }
    }
    
    verdict, suggestions = council._determine_final_verdict(council_results)
    assert verdict == "SUCCESS"
    
    # Case 2: Arbiter says FAILURE
    council_results["closed_source"]["arbiter"]["verdict"] = "FAILURE"
    council_results["closed_source"]["arbiter"]["suggestions"] = "Critical issues"
    
    verdict, suggestions = council._determine_final_verdict(council_results)
    assert verdict == "FAILURE"
    assert suggestions == "Critical issues"
    
    # Case 3: Arbiter says RETRY
    council_results["closed_source"]["arbiter"]["verdict"] = "RETRY"
    council_results["closed_source"]["arbiter"]["suggestions"] = "Fix these issues"
    council_results["closed_source"]["redactor"]["suggestions"] = "Refined suggestions"
    
    verdict, suggestions = council._determine_final_verdict(council_results)
    assert verdict == "RETRY"
    assert suggestions == "Refined suggestions"

@pytest.mark.vesper
def test_generate_changelog(temp_work_dir, sample_ledger, sample_config, monkeypatch):
    """Test changelog generation."""
    # Mock the get_llm_response function
    def mock_get_llm_response(*args, **kwargs):
        return "## Changes\n\n- Implemented feature X\n- Fixed bug Y"
    
    # Apply the monkeypatch
    import src.vesper_mind
    monkeypatch.setattr(src.vesper_mind, "get_llm_response", mock_get_llm_response)
    
    # Create VesperMind instance
    council = VesperMind(
        config=sample_config,
        ledger=sample_ledger,
        work_dir=temp_work_dir
    )
    
    # Set up a test run in the ledger
    run_id = sample_ledger.start_run("Test goal", 5, {})
    iteration_id = sample_ledger.start_iteration(run_id, 1, "Test prompt")
    sample_ledger.complete_iteration(
        run_id, iteration_id, "test diff", "test output", True, "SUCCESS", ""
    )
    
    # Generate changelog
    changelog = council.generate_changelog(run_id, iteration_id, "SUCCESS")
    
    # Check that changelog contains expected content
    assert "# Code Review" in changelog
    assert "Changes" in changelog
    assert "Implemented feature X" in changelog
    assert "Fixed bug Y" in changelog
