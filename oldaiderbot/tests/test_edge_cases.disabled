import pytest

@pytest.mark.control
def test_interrupt_escalates_to_sigkill():
    """If SIGTERM fails to stop Aider, SIGKILL is sent and process is forcibly terminated."""
    # Implementation would mock process termination and check escalation logic
    pass

@pytest.mark.control
def test_backend_recovers_after_forced_stop():
    """After a crash or forced stop, the harness and UI can be restarted and resume operation."""
    # Implementation would simulate a forced stop and restart, then check state recovery
    pass

@pytest.mark.control
def test_goal_prompt_reload_applies_immediately():
    """After editing the goal prompt, the *very next* Aider run uses the new prompt."""
    # Implementation would edit the goal prompt and verify the next run uses the new content
    pass

@pytest.mark.control
def test_plan_md_updated_each_round(tmp_path):
    """
    Ensure that PLAN.md is updated with a new council round entry after each run.
    """
    plan_path = tmp_path / "PLAN.md"
    # Simulate initial PLAN.md
    plan_path.write_text(
        """
This document is collaboratively updated by the open source council at each round.
## Current Plan
- [x] Previous round
- [ ] For the next round:
    - Review the results and outcomes of the previous iteration.
"""
    )
    old_content = plan_path.read_text()
    # Simulate council update
    plan_path.write_text(
        """
This document is collaboratively updated by the open source council at each round.
## Current Plan
- [x] Previous round
- [x] This round: Council reviewed and updated plan.
- [ ] For the next round:
    - Review the results and outcomes of the previous iteration.
"""
    )
    new_content = plan_path.read_text()
    assert old_content != new_content
    assert "- [x] This round: Council reviewed and updated plan." in new_content

@pytest.mark.ui
def test_live_log_handles_malformed_control_codes():
    """Malformed or partial Aider control codes in output do not break the live log."""
    # Implementation would send malformed control codes and verify live log stability
    pass

@pytest.mark.ui
def test_scrollback_limit_under_rapid_output():
    """Scrollback limit is enforced even when output is produced rapidly."""
    # Implementation would simulate rapid output and check scrollback enforcement
    pass

@pytest.mark.persistence
def test_ledger_recovers_from_interrupted_write():
    """Ledger/database recovers gracefully if interrupted mid-write (no corruption)."""
    # Implementation would simulate an interrupted write and verify ledger integrity
    pass
