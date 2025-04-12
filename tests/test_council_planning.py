import os
import shutil
import tempfile
import pytest
from pathlib import Path
import subprocess

@pytest.fixture
def temp_plan_and_goal(tmp_path):
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
    main_mod.council_planning_enforcement()
    updated_plan = Path("PLAN.md").read_text()
    assert "Council Round" in updated_plan
    assert "[Auto-generated placeholder" in updated_plan

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
    main_mod.council_planning_enforcement()
    updated_plan = Path("PLAN.md").read_text()
    assert "New actionable item" in updated_plan
    assert "Auto-generated" not in updated_plan
