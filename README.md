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

---
