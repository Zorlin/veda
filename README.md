# Veda: AI-Powered Software Development via an Intuitive Web UI

Veda aims to make software development accessible and efficient through an intelligent, user-friendly web interface. It orchestrates AI agents, primarily using Aider as its coding engine, to build and manage your projects based on your guidance.

**Core Focus:**

*   **User-Centric Web UI:** Veda's primary goal is to provide an exceptionally intuitive web interface that simplifies software creation and management for users of all technical levels. The UI guides users, clearly communicates Veda's actions, and makes the development process transparent and controllable.
*   **AI-Enhanced User Experience:** We are actively exploring how AI can be integrated *within* the UI to offer intelligent prompting assistance, suggest relevant tasks, and provide proactive guidance, making the interaction smoother and more effective.
*   **Seamless Aider Integration:** Veda manages Aider instances in the background. The focus is on how Aider's work is presented, managed, and refined through the user-friendly web interface.

Veda is not affiliated with Aider, but full credit to them for an excellent underlying coding engine.

---

## How it Works

1.  **Setup:** Provide API keys (like OpenRouter), ensure Ollama is running locally (for Veda's internal reasoning), and install Aider.
2.  **Interact via Web UI:** Launch Veda and open the web interface (default: http://localhost:9900).
3.  **Define Your Goal:** Use the web UI to describe your project goals. Veda may engage in a dialogue (powered by its own LLM) to clarify requirements before starting.
4.  **Veda Orchestrates:** Veda manages Aider agents to execute the development tasks, translating your goals into code.
5.  **Monitor & Refine:** Observe progress, review changes, and provide further instructions through the web UI. Veda aims to keep you informed and in control.

---

## Getting Started

Veda is designed to be a direct, action-oriented orchestrator. When you start Veda, it will ask for your project goal and then immediately begin working on it.

**What to expect:**
- When you start Veda, you'll be prompted to describe your project goal in a single input.
- Veda will immediately begin orchestrating agents to work on your project.
- You can interact with Veda at any time to provide more information, pause, or change direction.
- Files mentioned in your messages will be automatically read and analyzed.
- You can press Ctrl+D to detach from the interactive session; Veda will continue building in the background.

This streamlined process ensures that Veda quickly gets to work on your project while still keeping you in control throughout the development process.

For more details on Veda's orchestration philosophy, see [RULES.md](RULES.md).

## Prerequisites

*   **Python 3.9+**
*   **Git**
*   **Ollama:** Ensure Ollama is installed and running. Veda uses it for internal chat and coordination. See [ollama.com](https://ollama.com/).
*   **Aider:** Veda uses Aider as its primary coding engine. Install it using:
    ```bash
    python -m pip install aider-install
    aider-install
    ```
*   **OpenRouter API Key:** Aider will use models via OpenRouter. Set your API key as an environment variable:
    ```bash
    export OPENROUTER_API_KEY="your-api-key-here"
    ```
    You can add this to your `.bashrc`, `.zshrc`, or other shell configuration file. Veda will not start without this key.

## How to Install

Install Veda.
```
git clone https://github.com/zorlin/veda
cd veda
python -m pip install -r requirements.txt
```


## How to Use

1.  **Start Veda:**
    ```bash
    veda start
    ```
    This launches the Veda background service and the web server.

2.  **Open the Web Interface:**
    Navigate to `http://localhost:9900` (or the configured address) in your web browser. This is the primary way to interact with Veda.

3.  **Interact:**
    Use the web UI to:
    *   Define and refine project goals.
    *   Monitor the progress of AI agents.
    *   Review code changes.
    *   Chat with Veda for clarification or adjustments.

**Command-Line (Secondary):**

While the web UI is the main interface, some basic commands are available:

*   `veda`: Display help and status information.
*   `veda chat`: Engage in a text-based chat session with Veda (useful for quick interactions or if the web UI is unavailable).
*   `veda set instances <number|auto>`: Manually override the number of Aider instances Veda manages (use with caution, 'auto' is recommended).
*   `veda stop`: Stop the Veda service.
