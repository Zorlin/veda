import os
import pytest
import tempfile
from pathlib import Path
from unittest.mock import patch, MagicMock
import subprocess
import time

@pytest.fixture
def temp_plan_and_goal():
    """Create temporary PLAN.md and goal.prompt files for testing."""
    with tempfile.TemporaryDirectory() as temp_dir:
        # Create a temporary PLAN.md
        plan_path = Path(temp_dir) / "PLAN.md"
        with open(plan_path, "w") as f:
            f.write("""
# Project Plan

This document is collaboratively updated by the open source council at each round.
It contains the current, actionable plan for the next iteration(s) of the agent harness.

## Current Plan

- [x] The open source council will convene at each round to update this plan
- [ ] Implement feature A
- [ ] Fix bug B

## Council Summary & Plan for Next Round

*   **Summary of Last Round:** Initial planning round.
*   **Blockers/Issues:** None yet.
*   **Next Steps/Tasks:**
    *   [ ] First task
    *   [ ] Second task
*   **Reference:** This plan respects README.md goals.
""")
        
        # Create a temporary goal.prompt
        goal_path = Path(temp_dir) / "goal.prompt"
        with open(goal_path, "w") as f:
            f.write("""
make it so the open source council of AIs convenes each round to work on planning,
and together collaboratively update PLAN.md (very frequent)
and goal.prompt (rare, only for major shifts)
the high level goals set out in README.md should always be respected

Instructions:
- Modify main.py and tests to achieve these goals.
- At the end of each round, the open source council must review and update PLAN.md.
- Only update goal.prompt if a significant change in overall direction is required.
- All planning and actions must always respect the high-level goals in README.md.
""")
        
        # Create a temporary README.md
        readme_path = Path(temp_dir) / "README.md"
        with open(readme_path, "w") as f:
            f.write("""
# Project Goals

This project aims to build a self-improving AI system that:
1. Continuously learns and adapts
2. Maintains alignment with human values
3. Operates safely and transparently
""")
        
        # Change to the temporary directory
        original_dir = os.getcwd()
        os.chdir(temp_dir)
        
        yield {
            "plan_path": plan_path,
            "goal_path": goal_path,
            "readme_path": readme_path,
            "temp_dir": temp_dir
        }
        
        # Change back to the original directory
        os.chdir(original_dir)

def test_council_planning_enforcement_blocks_without_plan_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that the council planning enforcement blocks if PLAN.md is not updated,
    and auto-appends a new entry if not updated after two attempts.
    """
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout
    
    # Mock subprocess.run to avoid actually running tests
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: FakeCompleted())
    
    # Mock input to simulate user pressing Enter without updating the file
    monkeypatch.setattr("builtins.input", lambda: None)
    
    # Import the function after mocking
    from main import council_planning_enforcement
    
    # Get the initial content of PLAN.md
    with open(temp_plan_and_goal["plan_path"], "r") as f:
        initial_content = f.read()
    # Run the council planning enforcement with automated=False and testing_mode=True
    council_planning_enforcement(iteration_number=1, automated=False, testing_mode=True)

    # Check that PLAN.md was updated with an auto-appended entry
    with open(temp_plan_and_goal["plan_path"], "r") as f:
        updated_content = f.read()
    
    assert len(updated_content) > len(initial_content)
    assert "Council Round" in updated_content
    assert "[Auto-generated placeholder" in updated_content or "Auto-generated" in updated_content

def test_council_planning_enforcement_detects_plan_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that if PLAN.md is updated by the user, no auto-append occurs.
    """
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout
    
    # Mock subprocess.run to avoid actually running tests
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: FakeCompleted())
    
    # Create a counter to track input calls
    input_calls = [0]
    
    def mock_input():
        input_calls[0] += 1
        # On first call, update the PLAN.md file
        if input_calls[0] == 1:
            with open(temp_plan_and_goal["plan_path"], "a") as f:
                f.write("""
---

### Council Round 2 (2023-01-01 12:00:00)
*   **Summary of Last Round:** Made good progress.
*   **Blockers/Issues:** None.
*   **Next Steps/Tasks:**
    *   [ ] New task 1
    *   [ ] New task 2
*   **Reference:** This plan must always respect the high-level goals and constraints in README.md.
""")
        return None
    
    # Mock input to simulate user updating the file and pressing Enter
    monkeypatch.setattr("builtins.input", mock_input)
    
    # Import the function after mocking
    from main import council_planning_enforcement
    
    # Get the initial content of PLAN.md
    with open(temp_plan_and_goal["plan_path"], "r") as f:
        initial_content = f.read()

    # Run the council planning enforcement with automated=False and testing_mode=True
    council_planning_enforcement(iteration_number=1, automated=False, testing_mode=True)

    # Check that PLAN.md was updated with the user's entry and not auto-appended
    with open(temp_plan_and_goal["plan_path"], "r") as f:
        updated_content = f.read()
    
    assert "Council Round 2" in updated_content
    assert "Made good progress" in updated_content
    assert "Auto-generated placeholder" not in updated_content

def test_council_planning_enforcement_detects_goal_prompt_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that if goal.prompt is updated when a major shift is detected,
    it's properly handled.
    """
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout
    
    # Mock subprocess.run to avoid actually running tests
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: FakeCompleted())
    
    # Create a counter to track input calls
    input_calls = [0]
    
    def mock_input():
        input_calls[0] += 1
        # On first call, update the PLAN.md file with a major shift marker
        if input_calls[0] == 1:
            with open(temp_plan_and_goal["plan_path"], "a") as f:
                f.write("""
---

### Council Round 2 (2023-01-01 12:00:00)
*   **Summary of Last Round:** Detected a MAJOR_SHIFT in project direction.
*   **Blockers/Issues:** None.
*   **Next Steps/Tasks:**
    *   [ ] Implement new direction
    *   [ ] Update documentation
*   **Reference:** This plan must always respect the high-level goals and constraints in README.md.
""")
        # On second call, update the goal.prompt file
        elif input_calls[0] == 2:
            with open(temp_plan_and_goal["goal_path"], "w") as f:
                f.write("""
Updated goal prompt with new direction.
The open source council has determined a major shift is needed.

Instructions:
- Continue to update PLAN.md each round
- Focus on the new direction
- Respect README.md goals
""")
        return None
    
    # Mock input to simulate user updating files and pressing Enter
    monkeypatch.setattr("builtins.input", mock_input)
    
    # Import the function after mocking
    from main import council_planning_enforcement
    
    # Get the initial content of goal.prompt
    with open(temp_plan_and_goal["goal_path"], "r") as f:
        initial_goal = f.read()

    # Run the council planning enforcement with automated=False and testing_mode=True
    council_planning_enforcement(iteration_number=1, automated=False, testing_mode=True)

    # Check that goal.prompt was updated
    with open(temp_plan_and_goal["goal_path"], "r") as f:
        updated_goal = f.read()
    
    assert updated_goal != initial_goal
    assert "Updated goal prompt with new direction" in updated_goal

def test_council_planning_enforcement_handles_test_failures(monkeypatch, temp_plan_and_goal):
    """
    Verify that the council planning enforcement handles test failures correctly.
    """
    run_count = [0]
    
    def mock_run(*args, **kwargs):
        run_count[0] += 1
        
        class FakeCompleted:
            def __init__(self, returncode, stdout):
                self.returncode = returncode
                self.stdout = stdout
        
        # First test run fails, second one passes
        if run_count[0] == 1:
            return FakeCompleted(1, "Test failed: AssertionError: expected True but got False")
        else:
            return FakeCompleted(0, "All tests passed")
    
    # Mock subprocess.run to simulate test failures
    monkeypatch.setattr(subprocess, "run", mock_run)
    
    # Mock input to simulate user pressing Enter
    monkeypatch.setattr("builtins.input", lambda: None)
    
    # Import the function after mocking
    from main import council_planning_enforcement, update_goal_for_test_failures
    
    # Mock the update_goal_for_test_failures function to verify it's called
    original_update = update_goal_for_test_failures
    update_called = [False]
    
    def mock_update_goal(test_type):
        update_called[0] = True
        # Call the original to maintain behavior
        original_update(test_type)
    
    monkeypatch.setattr("main.update_goal_for_test_failures", mock_update_goal)
    
    # Run the council planning enforcement
    council_planning_enforcement(iteration_number=1, automated=True)
    
    # Verify that update_goal_for_test_failures was called
    assert update_called[0] == True
    
    # Verify that the test was retried and eventually passed
    assert run_count[0] > 1

def test_council_planning_integration_with_harness(monkeypatch, temp_plan_and_goal):
    """
    Test that council planning is properly integrated with the harness run cycle.
    """
    council_calls = []
    
    def mock_council_planning(iteration_number=None, test_failure_info=None, automated=True, testing_mode=False):
        council_calls.append({
            "iteration": iteration_number,
            "test_failure": bool(test_failure_info),
            "automated": automated,
            "testing_mode": testing_mode
        })
    
    # Mock the council planning function
    monkeypatch.setattr("main.council_planning_enforcement", mock_council_planning)
    
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout
    
    # Mock subprocess.run to avoid actually running tests
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: FakeCompleted())
    
    # Create a mock Harness class
    class MockHarness:
        def __init__(self, **kwargs):
            self.state = {"current_iteration": 0}
            self.run_called = False
            self.evaluate_called = False
        
        def run(self, initial_goal_prompt_or_file=None):
            self.run_called = True
            self.state["current_iteration"] = 3  # Simulate 3 iterations
            return "Success"
        
        def _evaluate_outcome(self, current_goal, aider_diff, pytest_output, pytest_passed):
            self.evaluate_called = True
            return "Success"
    
    # Import the necessary functions
    from main import run_with_council_planning, evaluate_with_council
    
    # Create a mock harness instance
    harness = MockHarness()
    
    # Store the original methods
    original_run = harness.run
    original_evaluate = harness._evaluate_outcome
    
    # Apply the patched methods
    harness.run = run_with_council_planning(harness, original_run)
    harness._evaluate_outcome = evaluate_with_council(harness, original_evaluate)
    
    # Run the harness
    harness.run("test_goal.prompt")
    
    # Verify that the council planning was called for initial and final planning
    assert len(council_calls) >= 2
    assert any(call["iteration"] == 0 for call in council_calls)  # Initial planning
    assert any(call["iteration"] == 3 for call in council_calls)  # Final planning
    
    # Now test the evaluation method
    harness._evaluate_outcome("test_goal", "test_diff", "test_output", True)
    
    # Verify that council planning was called during evaluation
    assert len(council_calls) >= 3
    
def test_update_goal_for_test_failures(monkeypatch, temp_plan_and_goal):
    """
    Test that the update_goal_for_test_failures function correctly updates goal.prompt
    when test failures are detected.
    """
    # Import the function
    from main import update_goal_for_test_failures
    
    # Get the initial content of goal.prompt
    with open(temp_plan_and_goal["goal_path"], "r") as f:
        initial_goal = f.read()
    
    # Update goal.prompt for pytest failures
    update_goal_for_test_failures("pytest")
    
    # Check that goal.prompt was updated with pytest guidance
    with open(temp_plan_and_goal["goal_path"], "r") as f:
        updated_goal = f.read()
    
    assert "fix the pytest test failures" in updated_goal.lower()
    
    # Reset goal.prompt
    with open(temp_plan_and_goal["goal_path"], "w") as f:
        f.write(initial_goal)
    
    # Update goal.prompt for cargo test failures
    update_goal_for_test_failures("cargo")
    
    # Check that goal.prompt was updated with cargo guidance
    with open(temp_plan_and_goal["goal_path"], "r") as f:
        updated_goal = f.read()
    
    assert "fix the cargo test failures" in updated_goal.lower()
