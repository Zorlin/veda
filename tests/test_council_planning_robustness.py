import pytest
import os
import tempfile
import shutil
from pathlib import Path
import time
from unittest.mock import patch, MagicMock
import threading
import fcntl

from main import (
    council_planning_enforcement,
    CouncilPlanningTestFailure,
    reload_file,
    get_file_mtime,
    update_goal_for_test_failures
)

@pytest.fixture
def temp_planning_files():
    """Create temporary PLAN.md, goal.prompt, and README.md files for testing."""
    # Create a temporary directory
    temp_dir = tempfile.mkdtemp()
    original_dir = os.getcwd()
    
    # Create the test files
    plan_path = Path(temp_dir) / "PLAN.md"
    goal_path = Path(temp_dir) / "goal.prompt"
    readme_path = Path(temp_dir) / "README.md"
    
    # Write initial content
    with open(plan_path, "w") as f:
        f.write("""
This document is collaboratively updated by the open source council at each round.
It contains the current, actionable plan for the next iteration(s) of the agent harness.

## Current Plan
- [ ] Test plan item 1
- [ ] Test plan item 2
""")
    
    with open(goal_path, "w") as f:
        f.write("Test goal prompt content")
    
    with open(readme_path, "w") as f:
        f.write("# Test README\nHigh-level goals and constraints")
    
    # Change to the temp directory
    os.chdir(temp_dir)
    
    yield {
        "plan_path": plan_path,
        "goal_path": goal_path,
        "readme_path": readme_path,
        "temp_dir": temp_dir
    }
    
    # Change back to the original directory and clean up
    os.chdir(original_dir)
    shutil.rmtree(temp_dir)

@pytest.mark.council_planning
def test_reload_file_retries_on_io_error(temp_planning_files):
    """Test that reload_file retries when encountering IO errors."""
    plan_path = temp_planning_files["plan_path"]
    
    # Create a mock open function that fails twice then succeeds
    original_open = open
    call_count = [0]
    
    def mock_open(*args, **kwargs):
        call_count[0] += 1
        if call_count[0] <= 2:
            raise IOError("Simulated IO error")
        return original_open(*args, **kwargs)
    
    # Apply the patch
    with patch("builtins.open", mock_open):
        # This should succeed on the third try
        content = reload_file(plan_path)
        
    # Verify we got the content
    assert "council" in content.lower()
    assert call_count[0] >= 3  # Should have tried at least 3 times

@pytest.mark.council_planning
def test_council_planning_handles_major_shift_markers(temp_planning_files):
    """Test that council planning detects various forms of major shift markers."""
    plan_path = temp_planning_files["plan_path"]
    
    # Test different variations of major shift markers
    markers = [
        "UPDATE_GOAL_PROMPT",
        "Major_Shift detected",
        "This represents a SIGNIFICANT CHANGE in direction",
        "The council has determined a direction change is needed"
    ]
    
    for marker in markers:
        # Add the marker to PLAN.md
        with open(plan_path, "w") as f:
            f.write(f"""
This document is collaboratively updated by the open source council at each round.
It contains the current, actionable plan for the next iteration(s) of the agent harness.

## Current Plan
- [ ] Test plan item 1
- [ ] Test plan item 2

## Note
{marker}
""")
        
        # Mock the LLM interaction to avoid actual API calls
        with patch("src.llm_interaction.get_llm_response", return_value="Updated goal prompt"):
            # Mock the console.print to capture output
            with patch("rich.console.Console.print") as mock_print:
                # Run in automated mode with testing_mode=True to avoid system exit
                council_planning_enforcement(
                    iteration_number=1,
                    automated=True,
                    testing_mode=True
                )
                
                # Check that the major shift was detected
                major_shift_detected = False
                for call_args in mock_print.call_args_list:
                    args = call_args[0]
                    if any("major shift" in str(arg).lower() for arg in args):
                        major_shift_detected = True
                        break
                
                assert major_shift_detected, f"Failed to detect major shift marker: {marker}"

@pytest.mark.council_planning
def test_update_goal_for_test_failures(temp_planning_files):
    """Test that update_goal_for_test_failures properly updates goal.prompt."""
    goal_path = temp_planning_files["goal_path"]
    
    # Get the original content
    original_content = reload_file(goal_path)
    
    # Update for pytest failures
    update_goal_for_test_failures("pytest")
    
    # Reload and check content
    updated_content = reload_file(goal_path)
    
    # Verify the update
    assert "fix the pytest test failures" in updated_content.lower()
    assert original_content in updated_content  # Original content should be preserved
    
    # Reset the file
    with open(goal_path, "w") as f:
        f.write(original_content)
    
    # Update for cargo failures
    update_goal_for_test_failures("cargo")
    
    # Reload and check content
    updated_content = reload_file(goal_path)
    
    # Verify the update
    assert "fix the cargo test failures" in updated_content.lower()
    assert original_content in updated_content  # Original content should be preserved

@pytest.mark.council_planning
def test_council_planning_creates_error_file_on_test_failure(temp_planning_files):
    """Test that council planning creates an error file when tests fail repeatedly."""
    # Mock subprocess.Popen to simulate test failures
    mock_process = MagicMock()
    mock_process.wait.return_value = 1  # Return code 1 indicates failure
    
    with patch("subprocess.Popen", return_value=mock_process):
        # Mock tempfile to use our controlled temp file
        with tempfile.NamedTemporaryFile(mode='w+', delete=False) as temp_file:
            temp_path = temp_file.name
            
            # Write some test failure output
            with open(temp_path, 'w') as f:
                f.write("Test failures: AssertionError: expected True but got False")
            
            # Mock open to return our controlled temp file content
            original_open = open
            
            def mock_open(*args, **kwargs):
                if args[0] == temp_path and 'r' in kwargs.get('mode', 'r'):
                    return original_open(temp_path, 'r')
                return original_open(*args, **kwargs)
            
            with patch("builtins.open", mock_open):
                # Run council planning with testing_mode=True to avoid system exit
                try:
                    council_planning_enforcement(
                        iteration_number=1,
                        automated=True,
                        testing_mode=True
                    )
                except CouncilPlanningTestFailure:
                    # Expected exception
                    pass
                
                # Check that the error file was created
                error_file = Path("COUNCIL_ERROR.md")
                assert error_file.exists(), "Error file was not created"
                
                # Check the content
                error_content = reload_file(error_file)
                assert "Test Failures" in error_content
                assert "AssertionError" in error_content
                
                # Clean up
                os.unlink(temp_path)
                os.unlink(error_file)

@pytest.mark.council_planning
def test_file_locking_during_updates(temp_planning_files):
    """Test that file locking works correctly during updates."""
    plan_path = temp_planning_files["plan_path"]
    
    # Function to simulate a concurrent write
    def concurrent_write():
        time.sleep(0.1)  # Small delay to ensure main thread has the lock
        try:
            with open(plan_path, "a") as f:
                # Try to get an exclusive lock with a short timeout
                try:
                    fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                    f.write("\nConcurrent write succeeded\n")
                    fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                    return True
                except IOError:
                    # Could not acquire lock
                    return False
        except Exception:
            return False
    
    # Start with a clean file
    with open(plan_path, "w") as f:
        f.write("Original content\n")
    
    # Start a thread that will try to write while we have the lock
    thread_result = [None]
    thread = threading.Thread(target=lambda: thread_result.__setitem__(0, concurrent_write()))
    
    # Open the file and hold the lock for a while
    with open(plan_path, "a") as f:
        # Acquire exclusive lock
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        
        # Start the concurrent thread
        thread.start()
        
        # Hold the lock for a moment
        time.sleep(0.3)
        
        # Write our content
        f.write("Main thread write\n")
        
        # Release the lock
        fcntl.flock(f.fileno(), fcntl.LOCK_UN)
    
    # Wait for the thread to complete
    thread.join()
    
    # Check if the concurrent write was blocked while we had the lock
    assert thread_result[0] is False, "Concurrent write should have been blocked"
    
    # Now try again after we've released the lock
    with open(plan_path, "a") as f:
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        f.write("Final write\n")
        fcntl.flock(f.fileno(), fcntl.LOCK_UN)
    
    # Verify the final content
    content = reload_file(plan_path)
    assert "Original content" in content
    assert "Main thread write" in content
    assert "Concurrent write succeeded" not in content
    assert "Final write" in content
