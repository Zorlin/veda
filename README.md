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
- *(Note: Closed-source Arbiter, Canonizer, Redactor roles are planned but not yet implemented).*

The council's collective judgment helps determine if the iteration was successful, needs refinement, or should be rejected.

The following is the full implementation plan, goal prompt, and an exhaustive test plan for evaluating its own correctness.

---

## ✅ Goal Prompt for Aider

```text
Your task is to build a Python-based test harness that:

1. Launches an Aider subprocess to apply a code or test change.
2. Runs pytest against the updated project.
3. Evaluates the outcome using a local LLM (via Ollama) that decides if the result was:
   - Successful
   - Retry-worthy with suggestions
   - A structural failure
4. Logs diffs, outcomes, and retry metadata in a stateful SQLite or JSON ledger.
5. Supports a prompt history chain so Aider can reason over its own history.
6. Continues looping until a 'converged' verdict is reached or max attempts.
7. Optionally allows another Aider process to act as a code reviewer.

You are allowed to modify files, install packages, and manage subprocesses.
This harness must be able to work on any project with a `pytest`-compatible test suite.
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
def test_llm_flags_invalid_or_unusable output():
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
- ✅ Agent-slot coordination system (one reviewer, one implementer)
- ✅ TUI or keyboard CLI interface for human-assisted nudges

---
