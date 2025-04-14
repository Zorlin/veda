# veda

Software development that doesn't sleep.

## how it works
* You provide API keys, an Ollama endpoint and a machine to run it on.
* You install Aider, Veda's thinking engine.
* You write a detailed prompt, or chat with Veda to refine your prompt and give it details and context.
* You start Veda in your project's directory.

Veda will run continuously, working on your project.

It will spin up, manage and watch multiple instances of Aider,
splitting up the work of creating your project,
and then merging and managing the results.

Veda is not affiliated with Aider, but full credit to them for an excellent project.

---

## Getting Started & Readiness

Veda is designed to be a thoughtful, collaborative orchestrator. Before it begins building, Veda will engage you in a readiness dialogue to ensure it fully understands your goals and that you are ready to proceed. This process is not hardcoded, but handled by Veda's LLM-based reasoning and dialogue.

**What to expect:**
- When you start Veda, it will ask clarifying questions and discuss your ideas.
- Veda will not proceed to build mode until it is convinced you are ready.
- You can interact with Veda at any time, even during build mode, to provide more information, pause, or change direction.
- Once Veda determines readiness (and confirms with you), it will enter build mode and begin orchestrating agents to work on your project.
- You can press Ctrl+D to detach from the interactive session; Veda will continue building in the background.

This readiness process ensures that Veda builds exactly what you want, and that you remain in control throughout the development process.

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


## how to use
```
veda
```

Running `veda` by itself will tell you more about Veda and how to use it.

```bash
veda start
```

Veda will run in the background automatically, 
and you can interact with it via the command line or via your web browser.

By default, it will manage how many instances it has running by itself,
but you can also set it manually.

```bash
veda set instances 10
```

If you want to let it manage itself, you can run this:
```bash
veda set instances auto
```


```bash
veda chat
```
Veda chat allows you to chat with Veda directly to refine what it's working on.

## web interface
Open http://localhost:9900 in your web browser to see the Veda web interface.
