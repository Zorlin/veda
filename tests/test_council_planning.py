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

@pytest.mark.council_planning
@patch('src.llm_interaction.get_llm_response', return_value="### Council Round 1 (Auto)\n* Summary: Auto-generated.\n* Next: [ ] Task.") # Mock LLM
def test_council_planning_enforcement_blocks_without_plan_update(mock_llm, monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that the council planning enforcement blocks if PLAN.md is not updated,
    and auto-appends a new entry if not updated after two attempts.
    """
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout

    # Mock subprocess.Popen to simulate successful test runs
    class MockPopen:
        def __init__(self, cmd, cwd, stdout, stderr, text):
            # Simulate writing "All tests passed" to the stdout file
            if hasattr(stdout, 'name'):
                with open(stdout.name, 'w') as f:
                    f.write("All tests passed.\n0 tests collected.") # Simulate pytest output
            self._returncode = 0 # Success

        def wait(self):
            return self._returncode

    monkeypatch.setattr(subprocess, "Popen", MockPopen)

    # Mock input to simulate user pressing Enter twice without updating the file
    input_calls = [0]
    def mock_input():
        input_calls[0] += 1
        return None
    monkeypatch.setattr("builtins.input", mock_input)

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
    assert "[Auto-generated placeholder" in updated_content or "Auto-generated" in updated_content or "Council Round 1 (Auto)" in updated_content # Check for mock LLM output

@pytest.mark.council_planning
def test_council_planning_enforcement_detects_plan_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that if PLAN.md is updated by the user, no auto-append occurs.
    """
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout

    # Mock subprocess.Popen to simulate successful test runs
    class MockPopen:
        def __init__(self, cmd, cwd, stdout, stderr, text):
             # Simulate writing "All tests passed" to the stdout file
            if hasattr(stdout, 'name'):
                with open(stdout.name, 'w') as f:
                    f.write("All tests passed.\n0 tests collected.") # Simulate pytest output
            self._returncode = 0 # Success

        def wait(self):
            return self._returncode

    monkeypatch.setattr(subprocess, "Popen", MockPopen)

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
    
    assert "Council Round 2" in updated_content # Check for user's update
    assert "Made good progress" in updated_content
    assert "Auto-generated placeholder" not in updated_content # Ensure auto-append didn't happen
    assert "Council Round 1 (Auto)" not in updated_content # Ensure mock LLM wasn't called unnecessarily

@pytest.mark.council_planning
@patch('src.llm_interaction.get_llm_response', return_value="Updated goal prompt via LLM.") # Mock LLM for potential auto goal update
def test_council_planning_enforcement_detects_goal_prompt_update(mock_llm, monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that if goal.prompt is updated when a major shift is detected,
    it's properly handled.
    """
    class FakeCompleted:
        def __init__(self, returncode=0, stdout="All tests passed"):
            self.returncode = returncode
            self.stdout = stdout

    # Mock subprocess.Popen to simulate successful test runs
    class MockPopen:
        def __init__(self, cmd, cwd, stdout, stderr, text):
             # Simulate writing "All tests passed" to the stdout file
            if hasattr(stdout, 'name'):
                with open(stdout.name, 'w') as f:
                    f.write("All tests passed.\n0 tests collected.") # Simulate pytest output
            self._returncode = 0 # Success

        def wait(self):
            return self._returncode

    monkeypatch.setattr(subprocess, "Popen", MockPopen)

    # Create a counter to track input calls and manage file updates
    input_calls = [0]
    plan_updated = False
    goal_updated = False

    def mock_input():
        call_count = input_calls[0]
        input_calls[0] += 1

        # First call: User updates PLAN.md with MAJOR_SHIFT marker and presses Enter
        if call_count == 0:
            nonlocal plan_updated
            if not plan_updated:
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
                plan_updated = True
            return None # Simulate pressing Enter after updating PLAN.md

        # Second call: Function detects MAJOR_SHIFT and asks user to confirm/update goal.prompt.
        # User updates goal.prompt and presses Enter.
        elif call_count == 1:
            nonlocal goal_updated
            if not goal_updated:
                with open(temp_plan_and_goal["goal_path"], "w") as f:
                    f.write("""
Updated goal prompt with new direction.
The open source council has determined a major shift is needed.

Instructions:
- Continue to update PLAN.md each round
- Focus on the new direction
- Respect README.md goals
""")
                goal_updated = True
            return None # Simulate pressing Enter after updating goal.prompt

        # Handle potential extra calls if logic changes
        else:
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
    
    assert updated_goal != initial_goal # Check goal was updated
    assert "Updated goal prompt with new direction" in updated_goal # Check user's update is present
    assert "Updated goal prompt via LLM." not in updated_goal # Ensure LLM wasn't used for goal update in manual mode

@pytest.mark.council_planning
@patch('main.update_goal_for_test_failures') # Mock the function directly
@patch('src.llm_interaction.get_llm_response', return_value="### Council Round 1 (Auto)\n* Summary: Auto-generated for test failure handling.\n* Next: [ ] Fix tests.") # Mock LLM for automated plan update
def test_council_planning_enforcement_handles_test_failures(mock_llm, mock_update_goal, monkeypatch, temp_plan_and_goal):
    """
    Verify that the council planning enforcement handles test failures correctly in automated mode.
    """
    run_count = [0]

    # Mock subprocess.Popen to simulate test failures then success
    class MockPopenHandlesFailure:
        def __init__(self, cmd, cwd, stdout, stderr, text):
            run_count[0] += 1
            self.stdout_path = stdout.name if hasattr(stdout, 'name') else None

            # First test run fails, subsequent ones pass (or fail depending on test logic)
            if run_count[0] == 1:
                self._returncode = 1 # Failure
                self.output_content = "Test failed: AssertionError: expected True but got False"
            else:
                # Subsequent calls in this test *should* still fail because automated=True
                # doesn't allow for manual fixes between retries. The function should raise
                # CouncilPlanningTestFailure after the first failure in automated mode.
                # Let's simulate failure again to match the expected behavior.
                self._returncode = 1 # Failure
                self.output_content = f"Test failed on attempt {run_count[0]}"

            # Simulate writing output to the temp file
            if self.stdout_path:
                with open(self.stdout_path, 'w') as f:
                    f.write(self.output_content)

        def wait(self):
            return self._returncode

    monkeypatch.setattr(subprocess, "Popen", MockPopenHandlesFailure)

    # No need to mock input for automated=True

    # Import the function after mocking
    from main import council_planning_enforcement, CouncilPlanningTestFailure

    # Run the council planning enforcement with testing_mode=True
    # Expect it to raise CouncilPlanningTestFailure because tests fail the first time
    # and automated mode doesn't wait for manual fixes.
    # The function should still call update_goal_for_test_failures before raising.
    with pytest.raises(CouncilPlanningTestFailure):
         council_planning_enforcement(iteration_number=1, automated=True, testing_mode=True)

    # Verify that update_goal_for_test_failures was called once after the first failure
    mock_update_goal.assert_called_once()
    # Check the type of failure passed (should be 'pytest' by default)
    assert mock_update_goal.call_args[0][0] == 'pytest'

    # Verify that the test command was run exactly once before the exception was raised
    # In automated=True, the function should raise after the first failure.
    assert run_count[0] == 1

@pytest.mark.council_planning
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
    
    # Verify that council planning was called during evaluation (should be called before original evaluate)
    assert len(council_calls) >= 3 # Initial, Final, Evaluate

@pytest.mark.council_planning
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
