import os
import pytest
import tempfile
import shutil
from pathlib import Path
from unittest.mock import patch, MagicMock

# Import the function to test
from main import update_goal_for_test_failures

@pytest.mark.resilience
def test_update_goal_for_test_failures_with_info():
    """Test that the goal prompt is updated with test failure information."""
    # Create a temporary directory for our test
    test_dir = tempfile.mkdtemp()
    
    try:
        # Create a temporary goal.prompt file
        goal_path = Path(test_dir) / "goal.prompt"
        with open(goal_path, "w", encoding="utf-8") as f:
            f.write("Original goal prompt content")
        
        # Mock the goal_prompt_path in the main module
        with patch("main.goal_prompt_path", goal_path):
            # Mock the logger and console
            with patch("main.logger") as mock_logger:
                with patch("main.console") as mock_console:
                    # Mock reload_file to return our test content
                    with patch("main.reload_file", return_value="Original goal prompt content"):
                        # Call the function with test failure info
                        test_failure_info = "Error: test_something failed\nAssertionError: expected True but got False"
                        update_goal_for_test_failures("pytest", test_failure_info)
                        
                        # Verify the console output
                        mock_console.print.assert_any_call("[bold green]Updated goal.prompt with guidance for pytest test failures.[/bold green]")
        
        # Read the updated file
        with open(goal_path, "r", encoding="utf-8") as f:
            updated_content = f.read()
        
        # Verify the content was updated correctly
        assert "## CRITICAL: Fix pytest test failures" in updated_content
        assert "Original goal prompt content" in updated_content
        assert "Failure details:" in updated_content
        assert "Error: test_something failed" in updated_content
        assert "AssertionError: expected True but got False" in updated_content
    
    finally:
        # Clean up
        shutil.rmtree(test_dir)

@pytest.mark.resilience
def test_update_goal_for_test_failures_without_info():
    """Test that the goal prompt is updated even without specific test failure information."""
    # Create a temporary directory for our test
    test_dir = tempfile.mkdtemp()
    
    try:
        # Create a temporary goal.prompt file
        goal_path = Path(test_dir) / "goal.prompt"
        with open(goal_path, "w", encoding="utf-8") as f:
            f.write("Original goal prompt content")
        
        # Mock the goal_prompt_path in the main module
        with patch("main.goal_prompt_path", goal_path):
            # Mock the logger and console
            with patch("main.logger") as mock_logger:
                with patch("main.console") as mock_console:
                    # Mock reload_file to return our test content
                    with patch("main.reload_file", return_value="Original goal prompt content"):
                        # Call the function without test failure info
                        update_goal_for_test_failures("cargo")
                        
                        # Verify the console output
                        mock_console.print.assert_any_call("[bold green]Updated goal.prompt with guidance for cargo test failures.[/bold green]")
        
        # Read the updated file
        with open(goal_path, "r", encoding="utf-8") as f:
            updated_content = f.read()
        
        # Verify the content was updated correctly
        assert "## CRITICAL: Fix cargo test failures" in updated_content
        assert "Original goal prompt content" in updated_content
        assert "Failure details:" not in updated_content  # No failure details should be included
    
    finally:
        # Clean up
        shutil.rmtree(test_dir)

@pytest.mark.resilience
def test_update_goal_skips_if_already_updated():
    """Test that the function skips updating if the goal prompt already contains guidance for the test type."""
    # Create a temporary directory for our test
    test_dir = tempfile.mkdtemp()
    
    try:
        # Create a temporary goal.prompt file with existing test failure guidance
        goal_path = Path(test_dir) / "goal.prompt"
        with open(goal_path, "w", encoding="utf-8") as f:
            f.write("Original content\n\nYou need to fix the pytest test failures")
        
        # Mock the goal_prompt_path in the main module
        with patch("main.goal_prompt_path", goal_path):
            # Mock the logger and console
            with patch("main.logger") as mock_logger:
                with patch("main.console") as mock_console:
                    # Mock reload_file to return our test content
                    with patch("main.reload_file", return_value="Original content\n\nYou need to fix the pytest test failures"):
                        # Call the function with test failure info
                        update_goal_for_test_failures("pytest", "Some test failure info")
                        
                        # Verify the logger output
                        mock_logger.info.assert_any_call("Goal prompt already contains guidance for pytest test failures.")
        
        # Read the file to verify it wasn't changed
        with open(goal_path, "r", encoding="utf-8") as f:
            content = f.read()
        
        # Verify the content wasn't changed
        assert content == "Original content\n\nYou need to fix the pytest test failures"
        assert "## CRITICAL: Fix pytest test failures" not in content
    
    finally:
        # Clean up
        shutil.rmtree(test_dir)

@pytest.mark.resilience
def test_update_goal_handles_errors():
    """Test that the function handles errors gracefully."""
    # Mock the goal_prompt_path in the main module to cause an error
    with patch("main.goal_prompt_path", "/nonexistent/path/that/doesnt/exist"):
        # Mock the logger and console
        with patch("main.logger") as mock_logger:
            with patch("main.console") as mock_console:
                # Mock reload_file to raise an exception
                with patch("main.reload_file", side_effect=Exception("Test exception")):
                    # Call the function
                    update_goal_for_test_failures("pytest", "Some test failure info")
                    
                    # Verify the error was logged
                    mock_logger.error.assert_called()
                    mock_console.print.assert_any_call("[bold red]Error updating goal.prompt: Test exception[/bold red]")
