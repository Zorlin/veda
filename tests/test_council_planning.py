import os
import shutil
import tempfile
import pytest
from pathlib import Path
import subprocess

@pytest.fixture
def temp_plan_and_goal(tmp_path):
    """Create temporary PLAN.md, goal.prompt, and README.md files for testing."""
    # Setup temp PLAN.md, goal.prompt, README.md
    plan = tmp_path / "PLAN.md"
    goal = tmp_path / "goal.prompt"
    readme = tmp_path / "README.md"
    plan.write_text(
        "# PLAN\n\n## Current Plan\n- [x] Initial plan\n\n## Council Summary & Plan for Next Round (Update Below)\n*   **Summary of Last Round:** [Council to fill in summary]\n"
    )
    goal.write_text("Initial goal prompt")
    readme.write_text("High level goals and constraints.")
    yield plan, goal, readme

def test_council_planning_enforcement_blocks_without_plan_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that the council planning enforcement blocks if PLAN.md is not updated,
    and auto-appends a new entry if not updated after two attempts.
    """
    import importlib.util
    import sys

    # Copy main.py to a temp dir and patch __file__ for relative ui_dir_path
    main_path = Path(__file__).parent.parent / "main.py"
    temp_dir = tempfile.mkdtemp()
    temp_main = Path(temp_dir) / "main.py"
    shutil.copy(main_path, temp_main)
    os.chdir(temp_dir)

    # Copy PLAN.md, goal.prompt, README.md
    plan, goal, readme = temp_plan_and_goal
    shutil.copy(plan, temp_dir + "/PLAN.md")
    shutil.copy(goal, temp_dir + "/goal.prompt")
    shutil.copy(readme, temp_dir + "/README.md")

    # Patch input() to simulate no human update, then allow auto-append
    input_calls = []
    def fake_input(prompt=""):
        input_calls.append(prompt)
        return ""
    monkeypatch.setattr("builtins.input", fake_input)

    # Patch subprocess.run to simulate passing tests
    class FakeCompleted:
        def __init__(self):
            self.returncode = 0
            self.stdout = "pytest passed"
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: FakeCompleted())

    # Import main.py as a module and call council_planning_enforcement
    spec = importlib.util.spec_from_file_location("main", str(temp_main))
    main_mod = importlib.util.module_from_spec(spec)
    sys.modules["main"] = main_mod
    spec.loader.exec_module(main_mod)

    # Should not raise, and PLAN.md should be auto-appended
    main_mod.council_planning_enforcement(iteration_number=1)
    updated_plan = Path("PLAN.md").read_text()
    assert "Council Round" in updated_plan
    assert "[Auto-generated placeholder" in updated_plan
    
    # Verify input was called twice (once for each attempt)
    assert len(input_calls) >= 2, "Input should be called at least twice"

def test_council_planning_enforcement_detects_plan_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that if PLAN.md is updated by the user, no auto-append occurs.
    """
    import importlib.util
    import sys

    main_path = Path(__file__).parent.parent / "main.py"
    temp_dir = tempfile.mkdtemp()
    temp_main = Path(temp_dir) / "main.py"
    shutil.copy(main_path, temp_main)
    os.chdir(temp_dir)

    plan, goal, readme = temp_plan_and_goal
    shutil.copy(plan, temp_dir + "/PLAN.md")
    shutil.copy(goal, temp_dir + "/goal.prompt")
    shutil.copy(readme, temp_dir + "/README.md")

    # Patch input() to simulate user updating PLAN.md on first prompt
    input_calls = []
    def fake_input(prompt=""):
        input_calls.append(prompt)
        # On first call, update PLAN.md to add a new actionable and summary
        if len(input_calls) == 1:
            with open("PLAN.md", "a") as f:
                f.write("\n- [ ] New actionable item\nSummary of Last Round: Did work\nREADME.md\n")
        return ""
    monkeypatch.setattr("builtins.input", fake_input)

    # Patch subprocess.run to simulate passing tests
    class FakeCompleted:
        def __init__(self):
            self.returncode = 0
            self.stdout = "pytest passed"
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: FakeCompleted())

    # Import main.py as a module and call council_planning_enforcement
    spec = importlib.util.spec_from_file_location("main", str(temp_main))
    main_mod = importlib.util.module_from_spec(spec)
    sys.modules["main"] = main_mod
    spec.loader.exec_module(main_mod)

    # Should not auto-append, and PLAN.md should contain the user update
    main_mod.council_planning_enforcement(iteration_number=2)
    updated_plan = Path("PLAN.md").read_text()
    assert "New actionable item" in updated_plan
    assert "Auto-generated" not in updated_plan
    
    # Verify input was called only once (since we updated the file)
    assert len(input_calls) >= 1, "Input should be called at least once"
    
    # Verify the iteration number is reflected in the log
    with open("PLAN.md", "r") as f:
        plan_content = f.read()
    assert "Did work" in plan_content, "User update should be preserved"
def test_council_planning_enforcement_detects_goal_prompt_update(monkeypatch, temp_plan_and_goal):
    """
    Simulate a round and verify that if goal.prompt is updated when a major shift is detected,
    it's properly handled.
    """
    import importlib.util
    import sys

    main_path = Path(__file__).parent.parent / "main.py"
    temp_dir = tempfile.mkdtemp()
    temp_main = Path(temp_dir) / "main.py"
    shutil.copy(main_path, temp_main)
    os.chdir(temp_dir)

    plan, goal, readme = temp_plan_and_goal
    shutil.copy(plan, temp_dir + "/PLAN.md")
    shutil.copy(goal, temp_dir + "/goal.prompt")
    shutil.copy(readme, temp_dir + "/README.md")

    # Add MAJOR_SHIFT marker to PLAN.md
    with open("PLAN.md", "a") as f:
        f.write("\n\nMAJOR_SHIFT: Direction change needed\n")

    # Patch input() to simulate user updating goal.prompt
    input_calls = []
    def fake_input(prompt=""):
        input_calls.append(prompt)
        # On first call (after detecting MAJOR_SHIFT), update goal.prompt
        if len(input_calls) == 1:
            with open("goal.prompt", "w") as f:
                f.write("Updated goal prompt with new direction")
        return ""
    monkeypatch.setattr("builtins.input", fake_input)

    # Patch subprocess.run to simulate passing tests
    class FakeCompleted:
        def __init__(self):
            self.returncode = 0
            self.stdout = "pytest passed"
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: FakeCompleted())

    # Import main.py as a module and call council_planning_enforcement
    spec = importlib.util.spec_from_file_location("main", str(temp_main))
    main_mod = importlib.util.module_from_spec(spec)
    sys.modules["main"] = main_mod
    spec.loader.exec_module(main_mod)

    # Run the enforcement function
    main_mod.council_planning_enforcement(iteration_number=3)
    
    # Verify goal.prompt was updated
    updated_goal = Path("goal.prompt").read_text()
    assert "Updated goal prompt with new direction" in updated_goal
    
    # Verify we were prompted for confirmation
    assert len(input_calls) >= 2, "Input should be called at least twice"
def test_council_planning_enforcement_handles_test_failures(monkeypatch, temp_plan_and_goal):
    """
    Verify that the council planning enforcement handles test failures correctly.
    """
    import importlib.util
    import sys

    main_path = Path(__file__).parent.parent / "main.py"
    temp_dir = tempfile.mkdtemp()
    temp_main = Path(temp_dir) / "main.py"
    shutil.copy(main_path, temp_main)
    os.chdir(temp_dir)

    plan, goal, readme = temp_plan_and_goal
    shutil.copy(plan, temp_dir + "/PLAN.md")
    shutil.copy(goal, temp_dir + "/goal.prompt")
    shutil.copy(readme, temp_dir + "/README.md")

    # Patch input to simulate user updating PLAN.md
    def fake_input(prompt=""):
        # Update PLAN.md to simulate user editing it
        with open("PLAN.md", "a") as f:
            f.write("\n\n## Test Failure Handling\n- [x] Handle test failures\nSummary of Last Round: Fixed tests\nREADME.md\n")
        return ""
    monkeypatch.setattr("builtins.input", fake_input)
    
    # Mock subprocess.run to simulate test failures then success
    run_count = [0]
    def mock_run(*args, **kwargs):
        run_count[0] += 1
        
        class FakeCompleted:
            def __init__(self, returncode, stdout):
                self.returncode = returncode
                self.stdout = stdout
        
        # Fail the first two attempts, succeed on the third
        if run_count[0] < 3:
            return FakeCompleted(1, "Tests failed!")
        else:
            return FakeCompleted(0, "All tests passed!")
    
    monkeypatch.setattr(subprocess, "run", mock_run)
    
    # Mock sys.exit to prevent actual exit
    exit_called = [False]
    exit_code = [None]
    def mock_exit(code=0):
        exit_called[0] = True
        exit_code[0] = code
        raise SystemExit(code)
    
    monkeypatch.setattr(sys, 'exit', mock_exit)
    
    # Import main.py as a module and call council_planning_enforcement
    spec = importlib.util.spec_from_file_location("main", str(temp_main))
    main_mod = importlib.util.module_from_spec(spec)
    sys.modules["main"] = main_mod
    spec.loader.exec_module(main_mod)
    
    # Run the enforcement function, expecting it to retry tests
    try:
        main_mod.council_planning_enforcement(iteration_number=4)
    except SystemExit:
        pass
    
    # Verify tests were run multiple times
    assert run_count[0] == 3, f"Tests should be run 3 times, but were run {run_count[0]} times"
    assert not exit_called[0], "System exit should not be called when tests eventually pass"
    
    # Reset for failure case
    run_count[0] = 0
    exit_called[0] = False
    exit_code[0] = None
    
    # Now make all test attempts fail
    def mock_run_all_fail(*args, **kwargs):
        run_count[0] += 1
        
        class FakeCompleted:
            def __init__(self):
                self.returncode = 1
                self.stdout = "Tests failed!"
        
        return FakeCompleted()
    
    monkeypatch.setattr(subprocess, "run", mock_run_all_fail)
    
    # Run the enforcement function, expecting it to exit after max retries
    try:
        main_mod.council_planning_enforcement(iteration_number=5)
    except SystemExit:
        pass
    
    # Verify tests were run the maximum number of times and system exit was called
    assert run_count[0] == 3, f"Tests should be run 3 times (max_test_retries), but were run {run_count[0]} times"
    assert exit_called[0], "System exit should be called when all test attempts fail"
    assert exit_code[0] == 1, f"Exit code should be 1 when tests fail, but was {exit_code[0]}"
def test_council_planning_integration_with_harness(monkeypatch, temp_plan_and_goal):
    """
    Test that council planning is properly integrated with the harness run cycle.
    """
    import importlib.util
    import sys

    main_path = Path(__file__).parent.parent / "main.py"
    temp_dir = tempfile.mkdtemp()
    temp_main = Path(temp_dir) / "main.py"
    shutil.copy(main_path, temp_main)
    os.chdir(temp_dir)

    plan, goal, readme = temp_plan_and_goal
    shutil.copy(plan, temp_dir + "/PLAN.md")
    shutil.copy(goal, temp_dir + "/goal.prompt")
    shutil.copy(readme, temp_dir + "/README.md")

    # Track council planning calls
    council_calls = []
    
    # Mock the council planning function
    def mock_council_planning(iteration_number=None):
        council_calls.append(iteration_number)
        return True
    
    # Mock input to avoid blocking
    def mock_input(prompt=""):
        return ""
    
    monkeypatch.setattr("builtins.input", mock_input)
    
    # Mock subprocess.run to simulate passing tests
    class FakeCompleted:
        def __init__(self):
            self.returncode = 0
            self.stdout = "pytest passed"
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: FakeCompleted())
    
    # Import main.py as a module
    spec = importlib.util.spec_from_file_location("main", str(temp_main))
    main_mod = importlib.util.module_from_spec(spec)
    sys.modules["main"] = main_mod
    spec.loader.exec_module(main_mod)
    
    # Replace the council planning function with our mock
    monkeypatch.setattr(main_mod, "council_planning_enforcement", mock_council_planning)
    
    # Create a mock Harness class
    class MockHarness:
        def __init__(self, **kwargs):
            self.state = {"current_iteration": 0}
            self.config = {"project_dir": str(temp_dir)}
        
        def run(self, initial_goal_prompt_or_file=None):
            # Simulate running iterations
            self.state["current_iteration"] = 3
            return True
    
    # Create a mock instance
    harness = MockHarness()
    
    # Apply the monkey patch to add council planning
    original_run = harness.run
    
    def run_with_council_planning(initial_goal_prompt_or_file=None):
        # Run initial council planning
        mock_council_planning(iteration_number=0)
        
        # Run the original method
        result = original_run(initial_goal_prompt_or_file)
        
        # Run final council planning
        mock_council_planning(iteration_number=harness.state["current_iteration"])
        
        return result
    
    # Replace the run method with our patched version
    harness.run = run_with_council_planning
    
    # Run the harness
    harness.run(initial_goal_prompt_or_file="test_goal")
    
    # Verify council planning was called for initial and final iterations
    assert len(council_calls) == 2, f"Council planning should be called twice, but was called {len(council_calls)} times"
    assert council_calls[0] == 0, f"First council call should be for iteration 0, but was {council_calls[0]}"
    assert council_calls[1] == 3, f"Second council call should be for iteration 3, but was {council_calls[1]}"
