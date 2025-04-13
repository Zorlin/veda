# 🤖 Aider Autoloop Harness: Self-Building Agent Framework

This is a self-bootstrapping agent harness built using Aider and a local LLM (via Ollama). It automates:
- Continuous agent-driven code generation
- Self-evaluation of outputs
- Pytest integration
- Retry cycles via local judgment
- VESPER.MIND council for evaluation (optional)
- Code review capabilities (optional)

## 🧠 VESPER.MIND Council

When enabled (`enable_council: true` in `config.yaml`), the harness utilizes a VESPER.MIND council for more robust evaluation of Aider's output. This council consists of multiple LLM personas, each analyzing the changes from a different perspective.

The current council configuration uses the following models (as defined in `config.yaml`):
- **Theorist:** `qwen2.5:14b` - Focuses on the conceptual soundness and alignment with the goal.
- **Architect:** `deepcoder:14b` - Examines code structure, design patterns, and maintainability.
- **Skeptic:** `gemma3:12b` - Challenges assumptions, looks for edge cases, and potential issues.
- **Historian:** `qwen2.5:14b` - Considers the changes in the context of the project's history and evolution.
- **Coordinator:** `command-r7b` - Synthesizes the inputs from other council members to provide a summary.
- *(Note: Closed-source Arbiter, Canonizer, Redactor roles are planned but not yet implemented. These roles would typically provide final judgment, integrate successful changes into a canonical representation, and potentially edit or refine the final output based on the council's findings).*

The council's collective judgment helps determine if the iteration was successful, needs refinement, or should be rejected.

The following is the full implementation plan, goal prompt, and an exhaustive test plan for evaluating its own correctness.

---

## ✅ Goal Prompt for Aider

```text
You are improving aiderbot according to the information laid out in README.md

Look for anything missing, any tests that can be added, anything you can do to make it run unstoppably and controllably

And expand on the concepts used if needed

You should:
1. Make Live Aider Output respect Aider's control codes (like \c for cancel)
2. Ensure scrollback never exceeds 10,000 lines to prevent browser crashes
3. Prevent text duplication in both the Live Log and Diff Viewers
4. Keep the Live Log focused on current state and recent activity
5. Implement a working interrupt system that actually stops Aider
6. Respect changes to goal.prompt even after initial run if they're edited

You should make sure:
- All output follows proper formatting control codes
- Diffs are displayed with clear syntax highlighting
- The Live Log only shows relevant, non-duplicated activity
- Interrupt process sends proper signals and cleans up resources
- Goal changes trigger reinitialization of Aider with new instructions

Expand testing to cover these new requirements and edge cases
```

---

#### 🚨 Edge Case & Robustness Tests

```python
@pytest.mark.control
def test_interrupt_escalates_to_sigkill():
    """If SIGTERM fails to stop Aider, SIGKILL is sent and process is forcibly terminated."""

@pytest.mark.control
def test_backend_recovers_after_forced_stop():
    """After a crash or forced stop, the harness and UI can be restarted and resume operation."""

@pytest.mark.control
def test_goal_prompt_reload_applies_immediately():
    """After editing the goal prompt, the *very next* Aider run uses the new prompt."""

@pytest.mark.control
def test_config_file_corruption_recovery():
    """If the config file is missing or corrupted, the harness should recover or recreate it."""

@pytest.mark.control
def test_ollama_or_aider_subprocess_loss_recovery():
    """If the Ollama or Aider subprocess crashes or is killed, the harness should detect and restart it."""

@pytest.mark.ui
def test_ui_server_reconnects_to_harness():
    """If the UI server is restarted, it should reconnect to the running harness and restore state."""

@pytest.mark.ui
def test_live_log_handles_malformed_control_codes():
    """Malformed or partial Aider control codes in output do not break the live log."""

@pytest.mark.ui
def test_scrollback_limit_under_rapid_output():
    """Scrollback limit is enforced even when output is produced rapidly."""

@pytest.mark.persistence
def test_ledger_recovers_from_interrupted_write():
    """Ledger/database recovers gracefully if interrupted mid-write (no corruption)."""

@pytest.mark.persistence
def test_ledger_recovers_from_disk_full():
    """If the disk is full or an I/O error occurs, the ledger should recover and resume operation."""

@pytest.mark.persistence
def test_no_duplicate_council_evaluations():
    """Ensure that duplicate council evaluations are not recorded for the same iteration."""
```

---

## 🧪 Exhaustive Test Plan

### 🏁 Bootstrap Tests

```python
@pytest.mark.bootstrap
def test_harness_initializes_config_directory():
    """Ensure the working directory, logs, and config files are initialized."""

@pytest.mark.bootstrap
def test_ollama_model_is_accessible():
    """Validate Ollama can be called and returns basic output."""

@pytest.mark.bootstrap
def test_aider_starts_and_receives_prompt():
    """Ensure Aider subprocess can be called with a test prompt."""
```

---

### ⚙️ Loop Mechanics

```python
@pytest.mark.loop
def test_aider_returns_diff_output():
    """Validate Aider returns non-empty code or patch diff."""

@pytest.mark.loop
def test_pytest_executes_after_diff():
    """Ensure pytest runs against updated files after each patch."""

@pytest.mark.loop
def test_local_llm_evaluates_result():
    """Check that Ollama gives a response based on pytest output."""

@pytest.mark.loop
def test_loop_retries_if_not_converged():
    """Harness must re-attempt improvement if Ollama says 'retry'."""
```

---

### 🧠 Local Evaluation Tests

```python
@pytest.mark.llm
def test_llm_handles_successful_output():
    """Ollama must correctly identify successful output from pytest logs."""

@pytest.mark.llm
def test_llm_handles_failed_output_and_suggests_retry():
    """When given failed output, LLM must respond with a retry plan."""

@pytest.mark.llm
def test_llm_flags_invalid_or_unusable_output():
    """If output is invalid Python or contradicts intent, LLM must block it."""
```

---

### 📚 Logging & Memory

```python
@pytest.mark.persistence
def test_diff_history_is_recorded():
    """All diffs must be saved per iteration to a history log."""

@pytest.mark.persistence
def test_outcomes_are_categorized_in_ledger():
    """Each run result must be labeled as pass/fail/blocked."""

@pytest.mark.persistence
def test_prompt_chain_can_be_reconstructed():
    """Prompt history must be reconstructible from logs or state DB."""
```

---

### ✨ UI, Control & Dynamic Goals

```python
@pytest.mark.ui
def test_aider_control_codes_are_handled():
    """Verify Aider output correctly interprets control codes (e.g., \c for cancel)."""

@pytest.mark.ui
def test_live_log_scrollback_limit():
    """Ensure the live log UI element enforces the maximum line limit."""

@pytest.mark.ui
def test_live_log_prevents_duplication():
    """Check that identical consecutive messages are not repeatedly added to the live log."""

@pytest.mark.ui
def test_diff_viewer_prevents_duplication():
    """Ensure diff viewers don't display duplicated content chunks."""

@pytest.mark.ui
def test_live_log_focuses_on_recent_activity():
    """Verify the live log primarily shows current status and recent events."""

@pytest.mark.ui
def test_diff_syntax_highlighting():
    """Check that code diffs are displayed with appropriate syntax highlighting."""

@pytest.mark.control
def test_interrupt_stops_aider_process():
    """Validate that the interrupt command successfully terminates the Aider subprocess."""

@pytest.mark.control
def test_interrupt_cleans_up_resources():
    """Ensure resources (threads, processes) are cleaned up after an interrupt (SIGTERM then SIGKILL)."""

@pytest.mark.control
def test_interrupt_stops_aider_promptly():
    """Verify that Aider stops processing quickly after an interrupt signal."""

@pytest.mark.control
def test_goal_prompt_changes_are_detected():
    """Verify that modifying the goal prompt file during a run is detected by the harness."""

@pytest.mark.control
def test_reloaded_goal_prompt_is_used():
    """Ensure that after a goal prompt reload, subsequent evaluations/retries use the new goal."""

```

---

### 🚦 Convergence Criteria

```python
@pytest.mark.convergence
def test_loop_stops_on_converged_success():
    """Harness should stop looping after clear success verdict."""

@pytest.mark.convergence
def test_loop_stops_on_max_retries():
    """If max retry count is reached, the loop should exit cleanly."""

@pytest.mark.convergence
def test_loop_detects_stuck_cycle_and_aborts():
    """Loop must detect non-progressing diffs and exit."""
```

---

### 🛑 Test Failure Annotation Checklist

- [ ] When a test fails, the goal is atomically updated *before* the next iteration.
- [ ] The annotation is inserted at the *very top* of the goal, using the required template.
- [ ] The annotation includes the reason, test output, and explicit instruction.
- [ ] The annotation is removed or updated as soon as the failure is resolved.
- [ ] The annotation update/removal is never skipped or delayed.
- [ ] This behavior is covered by automated tests.

---

## 🧩 Optional Features

- ✅ `@pytest.mark.mesh` → Simulate multiple Aider agents collaborating
- ✅ Realtime status logging to a web dashboard (Flask or Streamlit)
- ⬜ Agent-slot coordination system (one reviewer, one implementer)
- ⬜ TUI or keyboard CLI interface for human-assisted nudges
- ✅ Handle Aider control codes in live output (Backend sends codes; Frontend handles interpretation)
- ✅ Enforce scrollback limits in UI (Set to 10,000 lines)
- ✅ Prevent UI text duplication (Backend prevents duplicate raw chunks; Frontend handles rendered view)
- ✅ Implement robust interrupt mechanism (SIGTERM/SIGKILL sequence)
- ✅ Dynamically reload goal prompt changes (File hashing check)
- ✅ Edge-case and robustness tests for unstoppable, controllable operation

---

---
	
You are improving aiderbot according to the information laid out in README.md

Look for anything missing, any tests that can be added, anything you can do to make it run unstoppably and controllably

And expand on the concepts used if needed

ALL FUNCTIONALITY SHOULD HAVE EXTENSIVE TESTS.

You should make it so if aiderbot detects a bad test (bad cargo test or pytest)
it will modify the goal before the next iteration starts to specifically
tell the model to run cargo test

Conceptual Expansion:
- The harness should be self-healing, able to recover from failures at every layer (subprocess, config, UI, ledger, etc.), and continue operation with minimal human intervention.
- It should dynamically adapt to changes in the goal prompt or configuration, applying them immediately to allow the agent to evolve its behavior on the fly.
- All subprocesses (Aider, Ollama, Pytest) must be managed with robust signal handling and resource cleanup, ensuring that interrupts are always effective and no zombie processes remain.
- The VESPER.MIND council provides multi-perspective evaluation, reducing the risk of tunnel vision or single-point failure in judgment.
- Both backend and frontend logic must prevent duplicate output, keeping logs and diffs concise and relevant, and ensuring the UI remains performant and user-friendly.
- Strict scrollback limits must be enforced to prevent memory leaks and browser crashes, even under high-throughput output scenarios.
- The architecture should be extensible, supporting new council roles, agent slots, or UI modalities (TUI, CLI, web) with minimal changes to the core logic.

You should:
1. Make Live Aider Output respect Aider's control codes (like \c for cancel)
2. Prevent text duplication in both the Live Log and Diff Viewers
3. Keep the Live Log focused on current state and recent activity
4. Implement a working interrupt system that actually stops Aider
5. Respect changes to goal.prompt even after initial run if they're edited

You should make sure:
- All output follows proper formatting control codes
- Diffs are displayed with clear syntax highlighting
- The Live Log only shows relevant, non-duplicated activity
- Interrupt process sends proper signals and cleans up resources
- Goal changes trigger reinitialization of Aider with new instructions
