Awesome. Let’s write up the **ideal Veda workflow** as a clean, structured design—based on your notes—while respecting your principles (JSON, composability, GPU constraint, TUI control, etc.).

---

## 🌐 **Veda: Ideal Workflow (v1)**

Veda acts as a **meta-agent system**, guiding your local AI agents (Planner + Aider workers) through a structured and introspectable loop with a focus on **task clarity**, **agent coordination**, and **user control**.

---

### 🧠 1. **User Goal Prompting**
```plaintext
> Veda: What do you want to build?
```
- Veda starts by prompting the user for a natural-language **goal**.
- Any filenames mentioned are automatically read (as long as they are in the current directory or below).
- Veda parses this into a structured `goal.prompt`.

📁 Example output:
```json
{
  "goal": "I want to add login and session persistence using FastAPI and SQLite. You can look at app/main.py and auth/session.py."
}
```

---

### 📖 2. **File Awareness / Context Injection**
- Files mentioned in the prompt are loaded immediately (no `open()` required).
- File contents are stored in a JSON document alongside the goal:
```json
{
  "read_files": {
    "app/main.py": "...",
    "auth/session.py": "..."
  }
}
```

---

### 🧠 3. **Planning Phase (Ollama: DeepCoder:14B)**
- Once Veda confirms the goal is actionable, it spawns a **Planner process**:
  - Uses **DeepCoder:14B** via Ollama.
  - Reads `goal.prompt` and attached files.
  - Writes a **technical plan** as JSON: `goal.plan.json`

📁 Example:
```json
{
  "strategy": "Add authentication routes, create session store, update middleware.",
  "tasks": [
    { "file": "auth/session.py", "action": "add SQLite-backed session store" },
    { "file": "app/main.py", "action": "add login/logout routes" }
  ]
}
```

---

### ⚙️ 4. **Aider Worker Spawning**
- Veda spawns up to **4 parallel Aider agents**, each:
  - Uses `--yes --cache-prompts`
  - Takes the plan and a subset of the tasks.
  - Writes logs into `workflows/<worker-name>.json` in this format:
```json
{
  "worker": "worker-1",
  "status": "editing",
  "file": "auth/session.py",
  "summary": "Implemented session storage using sqlite3",
  "dependencies": ["app/main.py"]
}
```
- Workers can read logs from other agents to avoid collisions.

---

### 🧠 5. **Aider Response UX (Gemma3:12B)**
- Workers internally use **Gemma3:12B** to answer Aider prompts or questions.
- If a prompt needs answering, they:
  - Cache it
  - Answer automatically
  - Silently continue

---

### 🖥️ 6. **User Interaction (TUI)**
- Textual TUI allows:
  - ⌨️ Terminal-like tabs for each worker (`Terminal` widget)
    - Supports chatting, sending commands, or Ctrl-C to interrupt
  - 🏠 Home screen with:
    - Overview of all agents
    - “Broadcast note to all” (sends new goal/note to all workers)

---

### 🎩 7. **GPU Queue Management**
- Only **one Ollama GPU job runs at a time**
  - Veda maintains a polite queue for Ollama-bound jobs.
  - Workers wait until the GPU is available.

---

### 🧬 Summary JSON Schema Layouts

#### `goal.prompt`:
```json
{
  "goal": "Add FastAPI login routes with session persistence",
  "mentioned_files": ["app/main.py", "auth/session.py"]
}
```

#### `goal.plan.json`:
```json
{
  "strategy": "...",
  "tasks": [
    { "file": "x.py", "action": "..." }
  ]
}
```

#### `workflows/worker-N.json`:
```json
{
  "worker": "worker-N",
  "status": "editing",
  "file": "x.py",
  "summary": "added API route for login",
  "dependencies": ["y.py"]
}
```

# Veda: Software Development That Doesn't Sleep


Veda aims to make software development accessible and efficient through an intelligent, user-friendly web interface. It orchestrates AI agents, primarily using Aider as its coding engine, to build and manage your projects based on your guidance.

**Core Focus:**

*   **User-Centric TUI:** Veda's primary goal is to provide an exceptionally intuitive terminal interface that simplifies software creation and management for users of all technical levels. The TUI guides users, clearly communicates Veda's actions, and makes the development process transparent and controllable. The Web UI is a distant afterthought, provided only for edge cases and is not the main interface.
*   **AI-Enhanced User Experience:** We are actively exploring how AI can be integrated *within* the UI to offer intelligent prompting assistance, suggest relevant tasks, and provide proactive guidance, making the interaction smoother and more effective.
*   **Seamless Aider Integration:** Veda manages Aider instances in the background. The focus is on how Aider's work is presented, managed, and refined through the TUI.

Veda is not affiliated with Aider, but full credit to them for an excellent underlying coding engine.

---

## How it Works

1.  **Setup:** Provide API keys (like OpenRouter), ensure Ollama is running locally (for Veda's internal reasoning), and install Aider.
2.  **Interact via TUI:** Launch Veda and use the terminal interface (TUI) for all primary interactions. The TUI is the main way to describe your project goals, monitor progress, and control the orchestration. The web interface (default: http://localhost:9900) is available only as a fallback or for edge cases.
3.  **Define Your Goal:** Use the TUI to describe your project goals. Veda may engage in a dialogue (powered by its own LLM) to clarify requirements before starting.
4.  **Veda Orchestrates:** Veda manages Aider agents to execute the development tasks, translating your goals into code.
5.  **Monitor & Refine:** Observe progress, review changes, and provide further instructions through the TUI. Veda aims to keep you informed and in control.

---

## Getting Started

Veda is designed to be a direct, action-oriented orchestrator. When you start Veda, it will ask for your project goal and then immediately begin working on it.

**What to expect:**
- When you start Veda, you'll be prompted to describe your project goal in a single input (in the TUI).
- Veda will immediately begin orchestrating agents to work on your project.
- You can interact with Veda at any time via the TUI to provide more information, pause, or change direction.
- Files mentioned in your messages will be automatically read and analyzed.
- You can press Ctrl+D to detach from the interactive session; Veda will continue building in the background.

This streamlined process ensures that Veda quickly gets to work on your project while still keeping you in control throughout the development process.

For more details on Veda's orchestration philosophy, see [RULES.md](RULES.md).

## Prerequisites

*   **Python 3.10+**
*   **Git**
*   **Ollama:** Ensure Ollama is installed and running. Veda uses it for internal chat and coordination. See [ollama.com](https://ollama.com/).
*   **Aider:** Veda uses Aider as its primary coding engine. Install it using:
    ```bash
    python -m pip install aider-install
    aider-install
    ```
    By default, Veda will use the `openrouter/openai/gpt-4.1` model for Aider agents.
*   **Ollama:** Veda uses Ollama for evaluation, handoff, and meta-reasoning. The default Ollama model is `gemma3:12b`.
*   **OpenRouter API Key:** Aider will use models via OpenRouter. Set your API key as an environment variable:
    ```bash
    export OPENROUTER_API_KEY="your-api-key-here"
    ```
    You can add this to your `.bashrc`, `.zshrc`, or other shell configuration file. Veda will not start without this key.

## How to Install

Clone the repository and install Veda:

```bash
git clone https://github.com/zorlin/veda.git
cd veda
pip install --editable .
```


## How to Use

1.  **Start Veda:**
    ```bash
    veda start
    ```
    This launches the Veda background service and the web server.

2.  **(Optional) Open the Web Interface:**
    The web interface is available at `http://localhost:9900` (or the configured address) for edge cases or fallback use only. The TUI is the main interface.

3.  **Interact:**
    Use the TUI to:
    *   Define and refine project goals.
    *   Monitor the progress of AI agents.
    *   Review code changes.
    *   Chat with Veda for clarification or adjustments.

**Web UI (fallback only):**

The web UI is not the main interface and is only provided for rare fallback scenarios. All core features and controls are available in the TUI.

*   `veda`: Display help and status information.
*   `veda chat`: Engage in a text-based chat session with Veda (in the TUI; web UI is fallback only).
*   `veda set instances <number|auto>`: Manually override the number of Aider instances Veda manages (use with caution, 'auto' is recommended).
*   `veda stop`: Stop the Veda service.
