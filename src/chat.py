import logging
import sys
import os

# Allow finding constants.py when run from project root
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

try:
    import requests
except ImportError:
    print("Error: 'requests' library not found. Please install it: pip install requests")
    sys.exit(1)

from constants import OLLAMA_URL, VEDA_CHAT_MODEL, OPENROUTER_API_KEY

# Configure logging for this module
logger = logging.getLogger(__name__)

def ollama_chat(messages, model=VEDA_CHAT_MODEL, api_url=OLLAMA_URL):
    """Sends messages to the Ollama chat API and returns the response."""
    url = f"{api_url}/api/chat"
    payload = {
        "model": model,
        "messages": messages,
        "stream": False # Keep stream False for simple request/response
    }
    try:
        resp = requests.post(url, json=payload, timeout=60)
        resp.raise_for_status() # Raise an exception for bad status codes (4xx or 5xx)
        data = resp.json()
        response_message = data.get("message", {})
        # Ensure response_message is a dictionary before accessing 'content'
        if isinstance(response_message, dict):
            return response_message.get("content", "[No content in response message]")
        else:
            logger.error(f"Unexpected response format from Ollama. 'message' field is not a dict: {response_message}")
            return "[Error: Unexpected response format from Ollama]"
    except requests.exceptions.Timeout:
        logger.error(f"Timeout connecting to Ollama at {url}")
        return f"[Error: Timeout connecting to Ollama at {url}]"
    except requests.exceptions.ConnectionError:
        logger.error(f"Connection error connecting to Ollama at {url}. Is Ollama running?")
        return f"[Error: Connection error connecting to Ollama at {url}. Is Ollama running?]"
    except requests.exceptions.RequestException as e:
        logger.error(f"Error communicating with Ollama: {e}")
        return f"[Error communicating with Ollama: {e}]"
    except Exception as e:
        logger.error(f"An unexpected error occurred in ollama_chat: {e}", exc_info=True)
        return f"[Error: An unexpected error occurred: {e}]"


def chat_interface():
    """Runs the interactive command-line chat interface with Veda."""
    # Check for API key before starting chat (though chat uses Ollama, agents need it)
    if not OPENROUTER_API_KEY:
        print("Warning: OPENROUTER_API_KEY environment variable not set. Aider agents cannot be started.")
        # Allow chat to proceed, but warn the user.

    print("\nWelcome to Veda chat.")
    print("Ask about your project, give instructions, or type 'exit' to quit.")
    print(f"Connecting to Ollama at {OLLAMA_URL} using model {VEDA_CHAT_MODEL}")

    system_prompt = (
        "You are Veda, an advanced AI orchestrator for software development. "
        "You coordinate multiple specialized AI agents (architect, planner, developer, engineer, infra engineer, etc.) "
        "and personalities (theorist, architect, skeptic, historian, coordinator) to collaboratively build, improve, "
        "and maintain software projects. You use a common knowledge base (Postgres for deep knowledge, RAG via MCP server) "
        "and JSON files for inter-agent handoff. Your primary role in this chat is to understand the user's high-level goals, "
        "discuss requirements, determine readiness, and then initiate the appropriate agent handoffs. "
        "Engage in natural conversation. Ask clarifying questions. Confirm understanding. "
        "Do not start building until you are confident the user is ready and the goal is clear. "
        "Indicate readiness by suggesting the next step (e.g., 'Okay, I can ask the architect to design this.') "
        "and waiting for confirmation ('yes', 'proceed', 'go ahead', 'sounds good')."
        # "You can also provide status updates if asked about running agents (though the CLI/Web UI is better for that)."
        # Removed status update capability for now to keep focus on goal refinement.
    )

    messages = [{"role": "system", "content": system_prompt}]

    while True:
        try:
            msg = input("You: ")
        except EOFError: # Handle Ctrl+D
            print("\nExiting chat.")
            break

        if msg.strip().lower() == "exit":
            print("Exiting chat.")
            break

        if not msg.strip():
            continue

        messages.append({"role": "user", "content": msg})

        print("Veda (thinking)...")
        response = ollama_chat(messages) # Use the refactored function

        print(f"Veda: {response}")
        messages.append({"role": "assistant", "content": response})

        # Keep conversation history manageable (optional)
        # if len(messages) > 20: # Keep last ~10 turns + system prompt
        #     messages = [messages[0]] + messages[-20:]

# --- Readiness Chat Function (Used by `veda start` if no prompt) ---

def run_readiness_chat() -> str | None:
    """
    Conducts the initial readiness chat with the user to define the project goal.

    Returns:
        The user's confirmed initial goal prompt string, or None if the user exits.
    """
    print("\nWelcome to Veda. Let's define your project goal.")
    print(f"Connecting to Ollama at {OLLAMA_URL} using model {VEDA_CHAT_MODEL}")
    print("Describe what you want to build or change. Type 'exit' to cancel.")

    system_prompt = (
        "You are Veda, an AI orchestrator. Your current task is to help the user define their initial project goal. "
        "Ask clarifying questions to understand what they want to build or change. Discuss the requirements briefly. "
        "Once the goal seems clear, confirm it with the user. Ask something like: "
        "'Okay, so the goal is to [summarized goal]. Shall I start working on that?' or "
        "'Based on our discussion, the initial prompt would be: [prompt]. Is that correct and are you ready to proceed?'"
        "Wait for a clear confirmation (e.g., 'yes', 'proceed', 'that's correct', 'start'). "
        "Do NOT output the final prompt yourself. Your final message before the user confirms should be a question asking for confirmation."
    )
    messages = [{"role": "system", "content": system_prompt}]
    readiness_signals = [
        "yes", "yep", "yeah", "correct", "proceed", "go ahead", "start", "do it", "sounds good", "ok", "okay"
    ]
    last_user_msg = None

    while True:
        try:
            user_input = input("You: ")
        except EOFError:
            print("\nExiting setup.")
            return None

        if user_input.strip().lower() == "exit":
            print("Exiting setup.")
            return None

        if not user_input.strip():
            continue

        last_user_msg = user_input # Store the last thing the user said
        messages.append({"role": "user", "content": user_input})

        print("Veda (thinking)...")
        response = ollama_chat(messages)
        print(f"Veda: {response}")
        messages.append({"role": "assistant", "content": response})

        # Check if the *user's* last message was a confirmation signal *after* Veda asked for confirmation.
        # This is tricky without full state understanding in the LLM.
        # Let's simplify: If the user's input is primarily a readiness signal,
        # assume they are confirming the goal discussed in the previous turn.
        # We'll use the user's *previous* message as the potential prompt.

        # A better check: Did Veda's *last* response ask for confirmation?
        confirmation_phrases = ["shall i start", "is that correct", "ready to proceed", "confirm", "should i begin"]
        veda_asked_confirmation = any(phrase in response.lower() for phrase in confirmation_phrases)

        # Did the user *then* give a readiness signal?
        user_confirmed = any(signal == user_input.strip().lower() for signal in readiness_signals)

        if veda_asked_confirmation and user_confirmed:
            # Try to extract the goal from Veda's confirmation question or use the user's previous message.
            # This is still fragile. A more robust approach needs LLM to explicitly output the confirmed prompt.
            # For now, let's assume the user's message *before* the confirmation signal is the goal.
            # Find the user message before the last one.
            potential_prompt = None
            if len(messages) >= 4: # Need system, user, veda, user(confirmation)
                # The message before the last 'assistant' and last 'user' message
                if messages[-3]['role'] == 'user':
                    potential_prompt = messages[-3]['content']

            if potential_prompt:
                 print(f"\nVeda: Okay, proceeding with the goal: '{potential_prompt[:100]}...'")
                 return potential_prompt
            else:
                 # Fallback if we can't easily extract the prompt
                 print("\nVeda: Okay, proceeding. I'll use our conversation history to guide the agents.")
                 # We need *some* prompt for the AgentManager. Use the last user message before confirmation.
                 # This might not be ideal.
                 if last_user_msg:
                     print(f"(Using '{last_user_msg[:100]}...' as initial context)")
                     return last_user_msg
                 else:
                     print("Warning: Could not determine a specific goal prompt. Starting with a generic task.")
                     return "Develop the project based on the initial conversation."

        # Keep conversation history manageable (optional)
        # if len(messages) > 15: # Shorter history for readiness chat
        #     messages = [messages[0]] + messages[-15:]

if __name__ == '__main__':
    # Allow running chat independently for testing
    logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
    chat_interface()
