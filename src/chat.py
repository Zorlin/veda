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

def read_file_safely(filename, max_size=50*1024):
    """
    Safely read a file within the project directory.
    
    Args:
        filename: The name of the file to read
        max_size: Maximum file size in bytes (default: 50KB)
        
    Returns:
        The file content as a string, truncated if necessary
        
    Raises:
        FileNotFoundError: If the file doesn't exist
        SecurityException: If trying to access a file outside the project directory
        IOError: For other file reading errors
    """
    # Get absolute path of the file
    cwd = os.getcwd()
    full_path = os.path.abspath(os.path.join(cwd, filename))
    
    # Security check: ensure the file is within the project directory
    cwd_prefix = os.path.join(cwd, '')  # Ensures trailing separator
    if not full_path.startswith(cwd_prefix):
        raise SecurityException(f"Cannot access files outside the project directory: {filename}")
    
    # Check if file exists
    if not os.path.isfile(full_path):
        # Try case-insensitive search if the file is not found
        dir_path = os.path.dirname(full_path) or cwd
        base_name = os.path.basename(full_path)
        
        if os.path.isdir(dir_path):
            for entry in os.listdir(dir_path):
                if entry.lower() == base_name.lower():
                    # Found a case-insensitive match
                    full_path = os.path.join(dir_path, entry)
                    break
            else:
                # No match found even with case-insensitive search
                raise FileNotFoundError(f"File not found: {filename}")
        else:
            raise FileNotFoundError(f"File not found: {filename}")
    
    # Read file with size limit
    with open(full_path, 'r', encoding='utf-8', errors='ignore') as f:
        content = f.read(max_size + 1)  # Read slightly more to check if truncation needed
    
    # Check if truncation is needed
    if len(content) > max_size:
        truncated_content = content[:max_size] + "\n[... file truncated due to size limit ...]"
        return truncated_content
    
    return content

def detect_file_read_request(text):
    """
    Detect if the user is requesting to read a file.
    
    Args:
        text: The user's input text
        
    Returns:
        The filename if a read request is detected, None otherwise
    """
    read_patterns = [
        r"^read\s+([^\s]+)",
        r"^look at\s+([^\s]+)",
        r"^open\s+([^\s]+)",
        r"^cat\s+([^\s]+)",
        r"^show\s+([^\s]+)",
        r"^show me\s+([^\s]+)"
    ]
    
    text_lower = text.lower()
    for pattern in read_patterns:
        import re
        match = re.match(pattern, text_lower)
        if match:
            return match.group(1)
    
    return None

def get_file_completions(partial_path):
    """
    Get file completion suggestions for a partial path.
    
    Args:
        partial_path: The partial file path to complete
        
    Returns:
        A list of possible completions
    """
    import os
    import glob
    
    # Handle empty path
    if not partial_path:
        return []
    
    # Get the directory part and the file prefix
    if os.path.sep in partial_path:
        dir_part = os.path.dirname(partial_path)
        file_prefix = os.path.basename(partial_path)
        search_dir = os.path.join(os.getcwd(), dir_part)
    else:
        dir_part = ""
        file_prefix = partial_path
        search_dir = os.getcwd()
    
    # Make sure the directory exists
    if not os.path.isdir(search_dir):
        return []
    
    # Get all matching files and directories
    pattern = os.path.join(search_dir, f"{file_prefix}*")
    matches = glob.glob(pattern)
    
    # Format the results
    completions = []
    for match in matches:
        # Get the relative path from the current directory
        rel_path = os.path.relpath(match, os.getcwd())
        # Add a trailing slash for directories
        if os.path.isdir(match):
            rel_path += os.path.sep
        completions.append(rel_path)
    
    return sorted(completions)

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

    print("\nWelcome to Veda chat.", file=sys.stdout, flush=True)
    print("\nWelcome to Veda chat.", file=sys.stderr, flush=True)
    print("Ask about your project, give instructions, or type 'exit' to quit.", file=sys.stdout, flush=True)
    print("Ask about your project, give instructions, or type 'exit' to quit.", file=sys.stderr, flush=True)
    print("You can read files with 'read filename' and use tab completion for filenames.", file=sys.stdout, flush=True)
    print(f"Connecting to Ollama at {OLLAMA_URL} using model {VEDA_CHAT_MODEL}", file=sys.stdout, flush=True)
    print(f"Connecting to Ollama at {OLLAMA_URL} using model {VEDA_CHAT_MODEL}", file=sys.stderr, flush=True)

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
        "and waiting for confirmation ('yes', 'proceed', 'go ahead', 'sounds good'). "
        "You can read files when the user asks you to (e.g., 'read README.md', 'look at src/main.py'). "
        "Use the file contents to better understand the project and provide more informed responses."
        # "You can also provide status updates if asked about running agents (though the CLI/Web UI is better for that)."
        # Removed status update capability for now to keep focus on goal refinement.
    )

    messages = [{"role": "system", "content": system_prompt}]
    
    # Set up readline for tab completion
    try:
        import readline
        
        def complete(text, state):
            # This function is called by readline to get completion suggestions
            if text.lower().startswith(("read ", "look at ", "open ", "cat ", "show ", "show me ")):
                # Extract the command and partial filename
                parts = text.split(" ", 1)
                if len(parts) > 1:
                    command = parts[0]
                    partial_path = parts[1].strip()
                    
                    # Get completions for the partial path
                    completions = get_file_completions(partial_path)
                    
                    # Format completions with the command prefix
                    formatted_completions = [f"{command} {c}" for c in completions]
                    
                    # Return the state-th completion
                    if state < len(formatted_completions):
                        return formatted_completions[state]
            
            # No completions or all completions returned
            return None
        
        # Set the completer function
        readline.set_completer(complete)
        readline.parse_and_bind("tab: complete")
        
    except ImportError:
        print("Warning: readline module not available. Tab completion disabled.")

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
            
        # Check if the user is requesting to read a file
        filename = detect_file_read_request(msg.strip())
        if filename:
            try:
                file_content = read_file_safely(filename)
                print(f"2025-04-14 10:06:34,774 [INFO] Read content of '{filename}' for chat context.")
                
                # Add the user's original message
                messages.append({"role": "user", "content": msg})
                
                # Add file content as context in a separate message
                context_message = {
                    "role": "user", 
                    "content": f"Context: User asked to read '{filename}'. Here is the content:\n\n```\n{file_content}\n```"
                }
                messages.append(context_message)
            except FileNotFoundError:
                print(f"File not found: {filename}")
                messages.append({"role": "user", "content": msg})
                messages.append({
                    "role": "user", 
                    "content": f"[System note: User asked to read '{filename}', but it was not found. Inform the user.]"
                })
            except SecurityException as e:
                print(f"Security error: {e}")
                messages.append({"role": "user", "content": msg})
                messages.append({
                    "role": "user", 
                    "content": f"[System note: Access denied trying to read '{filename}'. {e} Inform the user.]"
                })
            except Exception as e:
                print(f"Error reading file: {e}")
                logger.error(f"Error reading file '{filename}': {e}")
                messages.append({"role": "user", "content": msg})
        else:
            # Regular message, no file reading
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
    print("You can read files with 'read filename' and use tab completion for filenames.")

    # Set up readline for tab completion
    try:
        import readline
        
        def complete(text, state):
            # This function is called by readline to get completion suggestions
            if text.lower().startswith(("read ", "look at ", "open ", "cat ", "show ", "show me ")):
                # Extract the command and partial filename
                parts = text.split(" ", 1)
                if len(parts) > 1:
                    command = parts[0]
                    partial_path = parts[1].strip()
                    
                    # Get completions for the partial path
                    completions = get_file_completions(partial_path)
                    
                    # Format completions with the command prefix
                    formatted_completions = [f"{command} {c}" for c in completions]
                    
                    # Return the state-th completion
                    if state < len(formatted_completions):
                        return formatted_completions[state]
            
            # No completions or all completions returned
            return None
        
        # Set the completer function
        readline.set_completer(complete)
        readline.parse_and_bind("tab: complete")
        
    except ImportError:
        print("Warning: readline module not available. Tab completion disabled.")

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
        # Use the new helper function to detect file read requests
        filename = detect_file_read_request(user_input.strip())
        
        # Start with base messages (system + current user input) for the next LLM call
        current_messages_for_llm = messages + [{"role": "user", "content": user_input}]
        system_note_for_llm = None
        file_read_success = False
        context_msg_content = None # Store content for history later

        if filename:
            try:
                # Use the new helper function to safely read files
                file_content = read_file_safely(filename)
                logger.info(f"Read content of '{filename}' for chat context.")
                print(f"2025-04-14 10:06:34,774 [INFO] Read content of '{filename}' for chat context.")
                
                # Prepare context message content
                context_msg_content = f"Context: User asked to read '{filename}'. Here is its content:\n\n```\n{file_content}\n```\n\nNow, please respond to the user's request: '{user_input}'"
                # Add this context as a new user message for the LLM call
                current_messages_for_llm.append({"role": "user", "content": context_msg_content})
                file_read_success = True
            except FileNotFoundError:
                logger.warning(f"User asked to read non-existent file: {filename}")
                system_note_for_llm = f"[System note: User asked to read '{filename}', but it was not found or is not a file. Please inform the user.]"
            except SecurityException as se:
                logger.error(f"SecurityException reading file '{filename}': {se}")
                system_note_for_llm = f"[System note: Access denied trying to read '{filename}'. Inform the user.]"
            except Exception as e:
                logger.error(f"Error reading file '{filename}': {e}")
                system_note_for_llm = f"[System note: An error occurred while trying to read '{filename}'. Inform the user.]"

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
