import pytest
import json
import os
from pathlib import Path
from unittest.mock import patch, MagicMock

from src.vesper_mind import VesperMind
from src.ledger import Ledger

# Module-level mock for get_llm_response to avoid repeated model checks
_llm_mock = None

@pytest.fixture(scope="module")
def mock_get_llm():
    """Create a module-scoped mock for get_llm_response."""
    global _llm_mock
    with patch('src.vesper_mind.get_llm_response') as mock:
        mock.return_value = "OK"
        _llm_mock = mock
        yield mock

@pytest.fixture
def temp_work_dir(tmp_path):
    """Create a temporary working directory for tests."""
    return tmp_path

@pytest.fixture
def mock_ledger(temp_work_dir):
    """Create a mock ledger for testing."""
    return Ledger(work_dir=temp_work_dir, storage_type="sqlite")

@pytest.fixture
def sample_config():
    """Sample configuration for testing."""
    return {
        "ollama_model": "gemma3:12b",
        "theorist_model": "qwen:14b",
        "architect_model": "deepseek-coder:16b",
        "skeptic_model": "gemma:7b",
        "historian_model": "yi:34b",
        "coordinator_model": "command-r-plus",
        "arbiter_model": "claude-3-sonnet",
        "canonizer_model": "gemini-2.5-pro",
        "redactor_model": "gpt-4-turbo"
    }

@pytest.mark.vesper
def test_vesper_council_initialization(mock_get_llm, temp_work_dir, mock_ledger, sample_config):
    """Test that the VESPER.MIND council initializes correctly."""
    # Initialize the council (mock_get_llm is already set up at module level)
    council = VesperMind(sample_config, mock_ledger, temp_work_dir)
    
    # Check that the council was initialized with the correct models
    assert council.open_source_council["theorist"]["model"] == "qwen:14b"
    assert council.open_source_council["architect"]["model"] == "deepseek-coder:16b"
    assert council.open_source_council["skeptic"]["model"] == "gemma:7b"
    assert council.open_source_council["historian"]["model"] == "yi:34b"
    assert council.open_source_council["coordinator"]["model"] == "command-r-plus"
    
    # Check that the closed-source council was initialized
    assert council.closed_source_council["arbiter"]["model"] == "claude-3-sonnet"
    assert council.closed_source_council["canonizer"]["model"] == "gemini-2.5-pro"
    assert council.closed_source_council["redactor"]["model"] == "gpt-4-turbo"
    
    # Check that the council directory was created
    assert (temp_work_dir / "council_outputs").exists()

@pytest.mark.vesper
def test_open_source_evaluation(mock_get_llm, temp_work_dir, mock_ledger, sample_config):
    """Test that the open-source evaluation works correctly."""
    # Update the mock for this specific test
    mock_get_llm.return_value = json.dumps({
        "evaluation": "This is a test evaluation",
        "score": 0.8,
        "concerns": ["Test concern"],
        "recommendations": ["Test recommendation"]
    })
    
    # Initialize the council
    council = VesperMind(sample_config, mock_ledger, temp_work_dir)
    
    # Run an open-source evaluation
    evaluation = council._run_open_source_evaluation(
        "theorist",
        "qwen:14b",
        "Test goal",
        "Test diff",
        "Test output",
        True,
        []
    )
    
    # Check that the evaluation was parsed correctly
    assert evaluation["evaluation"] == "This is a test evaluation"
    assert evaluation["score"] == 0.8
    assert evaluation["concerns"] == ["Test concern"]
    assert evaluation["recommendations"] == ["Test recommendation"]
    assert evaluation["role"] == "theorist"
    assert evaluation["model"] == "qwen:14b"
    assert evaluation["test_passed"] == True

@pytest.mark.vesper
def test_closed_source_evaluation(mock_get_llm, temp_work_dir, mock_ledger, sample_config):
    """Test that the closed-source evaluation works correctly."""
    # Update the mock for this specific test
    mock_get_llm.return_value = json.dumps({
        "evaluation": "This is a test evaluation",
        "score": 0.9,
        "verdict": "SUCCESS",
        "rationale": "Test rationale",
        "suggestions": "Test suggestions"
    })
    
    # Initialize the council
    council = VesperMind(sample_config, mock_ledger, temp_work_dir)
    
    # Run a closed-source evaluation
    evaluation = council._run_closed_source_evaluation(
        "arbiter",
        "gemma3:12b",
        "Test goal",
        "Test diff",
        "Test output",
        True,
        [],
        "Test summary"
    )
    
    # Check that the evaluation was parsed correctly
    assert evaluation["evaluation"] == "This is a test evaluation"
    assert evaluation["score"] == 0.9
    assert evaluation["verdict"] == "SUCCESS"
    assert evaluation["rationale"] == "Test rationale"
    assert evaluation["suggestions"] == "Test suggestions"
    assert evaluation["role"] == "arbiter"
    assert evaluation["model"] == "gemma3:12b"
    assert evaluation["test_passed"] == True

@pytest.mark.vesper
def test_synthesize_open_source_evaluations(mock_get_llm, temp_work_dir, mock_ledger, sample_config):
    """Test that the open-source evaluations are synthesized correctly."""
    # The mock is already set up at module level
    
    # Initialize the council
    council = VesperMind(sample_config, mock_ledger, temp_work_dir)
    
    # Create sample evaluations
    evaluations = {
        "theorist": {
            "evaluation": "Theorist evaluation",
            "score": 0.8,
            "concerns": ["Theorist concern"],
            "recommendations": ["Theorist recommendation"],
            "patterns_identified": ["Pattern 1", "Pattern 2"],
            "entropy_assessment": "Low entropy",
            "model": "qwen:14b",
            "role": "theorist",
            "test_passed": True
        },
        "architect": {
            "evaluation": "Architect evaluation",
            "score": 0.7,
            "concerns": ["Architect concern"],
            "recommendations": ["Architect recommendation"],
            "optimization_opportunities": ["Optimization 1"],
            "architectural_impact": "Minimal impact",
            "model": "deepseek-coder:16b",
            "role": "architect",
            "test_passed": True
        }
    }
    
    # Synthesize the evaluations
    synthesis = council._synthesize_open_source_evaluations(evaluations)
    
    # Check that the synthesis contains the expected information
    assert "# Open-Source Council Evaluation Summary" in synthesis
    assert "Average Score: 0.75" in synthesis
    assert "Theorist evaluation" in synthesis
    assert "Architect evaluation" in synthesis
    assert "Pattern 1" in synthesis
    assert "Pattern 2" in synthesis
    assert "Optimization 1" in synthesis
    assert "Theorist concern" in synthesis
    assert "Architect concern" in synthesis
    assert "Theorist recommendation" in synthesis
    assert "Architect recommendation" in synthesis

@pytest.mark.vesper
def test_determine_final_verdict(mock_get_llm, temp_work_dir, mock_ledger, sample_config):
    """Test that the final verdict is determined correctly."""
    # The mock is already set up at module level
    
    # Initialize the council
    council = VesperMind(sample_config, mock_ledger, temp_work_dir)
    
    # Create sample council results
    council_results = {
        "open_source": {
            "theorist": {
                "evaluation": "Theorist evaluation",
                "score": 0.8,
                "concerns": ["Theorist concern"],
                "recommendations": ["Theorist recommendation"],
                "test_passed": True
            },
            "architect": {
                "evaluation": "Architect evaluation",
                "score": 0.7,
                "concerns": ["Architect concern"],
                "recommendations": ["Architect recommendation"],
                "test_passed": True
            },
            "skeptic": {
                "evaluation": "Skeptic evaluation",
                "score": 0.9,
                "concerns": [],
                "recommendations": ["Skeptic recommendation"],
                "test_passed": True
            }
        },
        "closed_source": {
            "arbiter": {
                "evaluation": "Arbiter evaluation",
                "score": 0.9,
                "verdict": "SUCCESS",
                "rationale": "Arbiter rationale",
                "suggestions": "Arbiter suggestions",
                "critical_issues": []
            },
            "canonizer": {
                "evaluation": "Canonizer evaluation",
                "score": 0.9,
                "verdict": "SUCCESS",
                "rationale": "Canonizer rationale",
                "suggestions": "Canonizer suggestions",
                "version_tag": "v0.1.0-vesper"
            }
        }
    }
    
    # Determine the final verdict
    verdict, suggestions = council._determine_final_verdict(council_results)
    
    # Check that the verdict is SUCCESS
    assert verdict == "SUCCESS"
    assert "v0.1.0-vesper" in suggestions
    
    # Test with a FAILURE verdict
    council_results["closed_source"]["arbiter"]["verdict"] = "FAILURE"
    council_results["closed_source"]["arbiter"]["critical_issues"] = ["Critical issue"]
    
    verdict, suggestions = council._determine_final_verdict(council_results)
    
    # Check that the verdict is FAILURE
    assert verdict == "FAILURE"
    assert "Critical issue" in suggestions
    
    # Test with a RETRY verdict
    council_results["closed_source"]["arbiter"]["verdict"] = "RETRY"
    
    verdict, suggestions = council._determine_final_verdict(council_results)
    
    # Check that the verdict is RETRY
    assert verdict == "RETRY"
    assert "Arbiter suggestions" in suggestions

@pytest.mark.vesper
def test_generate_changelog(mock_get_llm, temp_work_dir, mock_ledger, sample_config, monkeypatch):
    """Test changelog generation."""
    # Update the mock for this specific test
    mock_get_llm.return_value = "## Test Changelog\n\nThis is a test changelog."
    
    # Mock the ledger.get_run_summary method
    def mock_get_run_summary(run_id):
        return {
            "initial_goal": "Test goal",
            "iterations": [
                {
                    "iteration_id": 1,
                    "iteration_number": 1,
                    "aider_diff": "Test diff",
                    "pytest_output": "Test output"
                }
            ]
        }
    
    monkeypatch.setattr(mock_ledger, "get_run_summary", mock_get_run_summary)
    
    # Mock the ledger.get_council_evaluations method
    def mock_get_council_evaluations(iteration_id):
        return []
    
    monkeypatch.setattr(mock_ledger, "get_council_evaluations", mock_get_council_evaluations)
    
    # Initialize the council
    council = VesperMind(sample_config, mock_ledger, temp_work_dir)
    
    # Generate a changelog
    changelog = council.generate_changelog(1, 1, "SUCCESS")
    
    # Check that the changelog was generated correctly
    assert "## Test Changelog" in changelog
    assert "This is a test changelog." in changelog
    
    # Check that the changelog file was created
    changelog_dir = temp_work_dir / "changelogs"
    assert changelog_dir.exists()
    assert (changelog_dir / "changelog_run1_iter1.md").exists()
