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

# Custom exception for security checks
class SecurityException(Exception):
    pass

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
        "If the user asks you to read a file (e.g., 'read README.md', 'look at src/utils.py'), the file content will be provided in the next message as context. Use that context to inform your response. "
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

        # --- File Reading Logic ---
        file_to_read = None
        read_request_triggers = ["read ", "look at ", "open ", "cat ", "show me "]
        words = user_input.lower().split()
        for trigger in read_request_triggers:
            if user_input.lower().startswith(trigger):
                # Find the potential filename after the trigger
                potential_filename = user_input[len(trigger):].strip().split()[0]
                # Basic validation: avoid absolute paths
                if not os.path.isabs(potential_filename):
                    # Let the later abspath/startswith check handle potential ".." traversal
                    file_to_read = potential_filename
                    break
            # Check for patterns like "read file <filename>"
            elif trigger.strip() in words:
                 try:
                     idx = words.index(trigger.strip())
                     if idx + 1 < len(words):
                         potential_filename = words[idx+1]
                         # Basic validation: avoid absolute paths
                         if not os.path.isabs(potential_filename):
                             # Let the later abspath/startswith check handle potential ".." traversal
                             file_to_read = potential_filename
                             break
                 except ValueError:
                     pass # Trigger not found

        # Start with base messages (system + current user input) for the next LLM call
        current_messages_for_llm = messages + [{"role": "user", "content": user_input}]
        system_note_for_llm = None
        file_read_success = False
        context_msg_content = None # Store content for history later

        if file_to_read:
            try:
                # Construct full path relative to current working directory (project root)
                full_path = os.path.abspath(os.path.join(os.getcwd(), file_to_read))
                # Security check: Ensure the resolved path is still within the project directory
                cwd = os.getcwd()
                logger.debug(f"Security Check: Resolved path='{full_path}', CWD='{cwd}'")
                # Ensure the CWD path used for comparison has a trailing separator
                # This prevents accepting '/tmp/proj/file' if CWD is '/tmp/pro'
                cwd_prefix = os.path.join(cwd, '') # Ensures trailing separator
                if not full_path.startswith(cwd_prefix):
                    logger.warning(f"Security Check FAILED: Path '{full_path}' is outside CWD '{cwd_prefix}'.")
                    raise SecurityException(f"Access denied: Path '{file_to_read}' is outside the project directory.")
                else:
                    logger.debug("Security Check PASSED: Path is inside CWD.")

                if os.path.isfile(full_path):
                    with open(full_path, 'r', encoding='utf-8', errors='ignore') as f:
                        # Limit file size to avoid overwhelming the context window (e.g., 50KB)
                        max_size = 50 * 1024
                        file_content = f.read(max_size)
                        if len(file_content) == max_size:
                            file_content += "\n[... file truncated due to size limit ...]"
                        logger.info(f"Read content of '{file_to_read}' for chat context.")
                        # Prepare context message content
                        context_msg_content = f"Context: User asked to read '{file_to_read}'. Here is its content:\n\n```\n{file_content}\n```\n\nNow, please respond to the user's request: '{user_input}'"
                        # Add this context as a new user message for the LLM call
                        current_messages_for_llm.append({"role": "user", "content": context_msg_content})
                        file_read_success = True
                else:
                    logger.warning(f"User asked to read non-existent file: {file_to_read}")
                    system_note_for_llm = f"[System note: User asked to read '{file_to_read}', but it was not found or is not a file. Please inform the user.]"

            except SecurityException as se:
                 logger.error(f"SecurityException reading file '{file_to_read}': {se}")
                 system_note_for_llm = f"[System note: Access denied trying to read '{file_to_read}'. Inform the user.]"
            except Exception as e:
                logger.error(f"Error reading file '{file_to_read}': {e}")
                system_note_for_llm = f"[System note: An error occurred while trying to read '{file_to_read}'. Inform the user.]"

            # If a system note was generated due to an error/warning, add it for the LLM
            if system_note_for_llm:
                 current_messages_for_llm.append({"role": "user", "content": system_note_for_llm})
        # --- End File Reading Logic ---


        print("Veda (thinking)...")
        # Use the potentially modified message list for the LLM call
        response = ollama_chat(current_messages_for_llm)
        print(f"Veda: {response}")

        # Add messages to the persistent history
        messages.append({"role": "user", "content": user_input}) # Add original user message
        if file_read_success and context_msg_content: # If we added context successfully
             messages.append({"role": "user", "content": context_msg_content}) # Add the context to history
        elif system_note_for_llm: # If there was a system note (error/warning)
             messages.append({"role": "user", "content": system_note_for_llm}) # Add the note to history
        messages.append({"role": "assistant", "content": response}) # Add Veda's response

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
            # Find the user message before the last 'assistant' response and the last 'user' confirmation.
            potential_prompt = None
            # Iterate backwards from the message before the last assistant response
            for i in range(len(messages) - 3, 0, -1):
                if messages[i]['role'] == 'user':
                    potential_prompt = messages[i]['content']
                    # Check if this prompt is actually a system note or context message
                    is_system_note = potential_prompt.startswith("[System note:")
                    is_context_msg = potential_prompt.startswith("Context: User asked to read")

                    if is_system_note or is_context_msg:
                        # If it is, try to get the *actual* user message before it
                        if i > 0 and messages[i-1]['role'] == 'user':
                            potential_prompt = messages[i-1]['content']
                        else:
                            # Should not happen if history is built correctly, but handle defensively
                            potential_prompt = None # Reset if we can't find the original user message
                    break # Found the relevant message (or the note/context derived from it)

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
