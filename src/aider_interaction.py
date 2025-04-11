import logging
import pexpect # Use pexpect for interactive control
import re
import shlex # Import shlex for quoting
import sys
from typing import List, Dict, Tuple, Optional, Any

# Import the LLM interaction function
from .llm_interaction import get_llm_response

# Configure logging for this module
logger = logging.getLogger(__name__)

# --- Constants for Interaction ---
# Regex patterns to detect common Aider prompts requiring a response
# Adjust these based on the exact prompts Aider uses
APPLY_PROMPT_PATTERN = r"Apply changes\? \[y/n/q/a/v\]" # Example, adjust as needed
PROCEED_PROMPT_PATTERN = r"Proceed\? \[y/n\]" # Example
AIDER_PROMPT_PATTERNS = [
    APPLY_PROMPT_PATTERN,
    PROCEED_PROMPT_PATTERN,
    # Add other patterns if Aider has more interactive prompts
]
# Default timeout for waiting for Aider output
AIDER_TIMEOUT = 300 # seconds (5 minutes)

# --- Helper Function for LLM Response ---

def _get_llm_aider_response(
    aider_output: str,
    prompt_pattern: str,
    config: Dict[str, Any],
    history: List[Dict[str, str]]
) -> str:
    """
    Uses the LLM to decide how to respond to an Aider prompt.
    """
    system_prompt = """You are an automated agent controlling the 'aider' coding tool.
Aider has presented a prompt requiring a decision. Analyze the preceding output, focusing on any proposed code changes (diffs).
Based on the overall goal (implicitly from history) and the specific changes proposed, decide the appropriate response.
Respond ONLY with the single character representing the desired action (e.g., 'y', 'n', 'q', 'a')."""

    # Extract the specific question Aider asked (the matched pattern)
    match = re.search(prompt_pattern, aider_output, re.IGNORECASE | re.MULTILINE)
    aider_question = match.group(0) if match else f"Detected prompt matching: {prompt_pattern}"

    llm_prompt = f"""Aider Output Before Prompt:
```
{aider_output}
```

Aider Prompt: "{aider_question}"

Based on the output and the prompt, what is the best response?
Choose ONLY one character from the options provided in the prompt (e.g., y, n, q, a, v).
Response:"""

    try:
        # Use a limited history? Or the full history? Let's try full for now.
        response = get_llm_response(llm_prompt, config, history, system_prompt=system_prompt)
        # Validate the response - ensure it's one of the expected single characters
        # Extract allowed characters from the prompt pattern (e.g., y/n/q/a/v)
        allowed_chars_match = re.search(r'\[([a-z/]+)\]', aider_question, re.IGNORECASE)
        allowed_chars = set()
        if allowed_chars_match:
            allowed_chars = set(c for c in allowed_chars_match.group(1) if c != '/')

        logger.debug(f"Raw LLM response for Aider prompt: '{response}'")
        # Basic validation: take the first character, lowercased
        llm_choice = response.strip().lower()[:1]

        if allowed_chars and llm_choice not in allowed_chars:
            logger.warning(f"LLM proposed invalid response '{llm_choice}' (from raw: '{response}'). Allowed: {allowed_chars}. Defaulting to 'n'.")
            return 'n' # Default to 'no' if LLM response is invalid
        elif not allowed_chars and llm_choice not in ('y', 'n', 'q', 'a', 'v'): # Fallback validation
             logger.warning(f"LLM proposed potentially invalid response '{response}'. Defaulting to 'n'.")
             return 'n'

        logger.info(f"LLM decided response to '{aider_question}' is: '{llm_choice}'")
        return llm_choice

    except Exception as e:
        logger.error(f"Error getting LLM response for Aider prompt: {e}. Defaulting to 'n'.")
        return 'n' # Default to 'no' on LLM error

# --- Main Aider Interaction Function ---

def run_aider(
    prompt: str,
    config: Dict[str, Any],
    history: List[Dict[str, str]],
    work_dir: str,
) -> Tuple[Optional[str], Optional[str]]:
    """
    Runs the Aider subprocess interactively using pexpect.

    Args:
        prompt: The initial user prompt for Aider.
        config: The harness configuration dictionary.
        history: The conversation history (used for LLM context).
        work_dir: The directory where Aider should run.

    Returns:
        A tuple containing:
        - The final extracted diff string (if successful).
        - An error message string (if an error occurred).
    """
    aider_command = config.get("aider_command", "aider")
    # Quote the prompt to handle spaces and special characters
    quoted_prompt = shlex.quote(prompt)
    # Remove --yes, add --message with the quoted initial prompt
    command_args = [
        f"--message {quoted_prompt}", # Pass the quoted prompt as part of the argument
        # Add other necessary aider flags from config if needed
        # e.g., "--model", config.get("aider_model", "gpt-4")
    ]
    # Add the working directory itself as an argument for Aider to scan
    command_args.append(shlex.quote(work_dir))
    # Construct the command string carefully
    full_command = f"{aider_command} {' '.join(command_args)}"

    logger.info(f"Spawning Aider command: {full_command} in {work_dir}")
    # Log the prompt separately for clarity, avoiding potential quoting issues in logs
    logger.debug(f"Aider initial prompt content:\n{prompt}")
    full_output = "" # Accumulate all output from the session

    try:
        # Spawn the process
        child = pexpect.spawn(
            full_command,
            cwd=work_dir,
            encoding='utf-8',
            timeout=AIDER_TIMEOUT, # Overall timeout for the whole command
            logfile=sys.stdout # Log output to stdout for visibility (optional)
        )

        # Interaction loop
        while True:
            try:
                # Wait for either a known prompt pattern or EOF/Timeout
                index = child.expect(AIDER_PROMPT_PATTERNS + [pexpect.EOF, pexpect.TIMEOUT], timeout=AIDER_TIMEOUT)
                full_output += child.before + child.after # Accumulate output

                if index < len(AIDER_PROMPT_PATTERNS):
                    # Matched one of the known prompts
                    matched_pattern = AIDER_PROMPT_PATTERNS[index]
                    logger.info(f"Aider prompt detected matching pattern: {matched_pattern}")

                    # Get LLM response
                    response_char = _get_llm_aider_response(
                        aider_output=full_output, # Pass accumulated output
                        prompt_pattern=matched_pattern,
                        config=config,
                        history=history # Pass conversation history
                    )

                    # Send the response back to Aider
                    logger.info(f"Sending response '{response_char}' to Aider.")
                    child.sendline(response_char)

                    # Handle specific responses if needed (e.g., 'q' means quit)
                    if response_char == 'q':
                        logger.warning("LLM chose to quit Aider ('q'). Terminating interaction.")
                        child.close()
                        return None, "Aider interaction terminated by LLM ('q')"

                elif index == len(AIDER_PROMPT_PATTERNS): # EOF
                    logger.info("Aider process finished (EOF detected).")
                    break # Exit loop, process finished normally
                elif index == len(AIDER_PROMPT_PATTERNS) + 1: # Timeout
                    logger.error(f"Timeout waiting for Aider output after {AIDER_TIMEOUT} seconds.")
                    full_output += child.before # Capture output before timeout
                    child.close(force=True)
                    return None, f"Timeout waiting for Aider after {AIDER_TIMEOUT}s"

            except pexpect.exceptions.ExceptionPexpect as e:
                 logger.error(f"Pexpect error during Aider interaction: {e}")
                 logger.debug(f"Output before error:\n{full_output}")
                 if child.isalive():
                     child.close(force=True)
                 return None, f"Pexpect error: {e}"

        # --- Process finished, analyze output ---
        if child.exitstatus != 0:
            # Check if closed gracefully or crashed
            if child.signalstatus:
                 error_message = f"Aider command terminated unexpectedly by signal: {child.signalstatus}.\nOutput:\n{full_output}"
            else:
                 error_message = f"Aider command failed with exit code {child.exitstatus}.\nOutput:\n{full_output}"
            logger.error(error_message)
            return None, error_message

        logger.info(f"Aider command finished successfully (exit code {child.exitstatus}).")

        # --- Diff Extraction Logic (applied to full_output) ---
        # Look for the last diff block in the entire session output
        diff_output = None
        last_diff_start = full_output.rfind("```diff")
        if last_diff_start != -1:
            # Find the end marker after the last start marker
            last_diff_end = full_output.find("```", last_diff_start + 7)
            if last_diff_end != -1:
                diff_output = full_output[last_diff_start + 7 : last_diff_end].strip()
                logger.info("Extracted last diff block from Aider session output.")

        if diff_output is None:
            # Fallback or alternative strategy if no diff block found
            logger.warning("Could not find standard ```diff ... ``` block in Aider session output.")
            # Decide what to return - maybe empty string indicates no changes applied?
            # Or maybe check for specific "No changes applied" messages from Aider?
            if "No changes applied." in full_output or "No changes needed." in full_output:
                 logger.info("Aider indicated no changes were applied.")
                 return "", None # Success, no changes
            else:
                 # Unclear if changes happened but diff wasn't found
                 logger.warning("Assuming no changes as diff block wasn't found and no explicit 'no changes' message detected.")
                 return "", None # Treat as success with no changes for now

        if not diff_output:
             logger.warning("Extracted diff block is empty.")
             return "", None # Success, no changes (empty diff)

        return diff_output, None # Return extracted diff, no error

    except pexpect.exceptions.ExceptionPexpect as e:
        # Errors during spawn (e.g., command not found)
        error_message = f"Failed to spawn Aider command '{full_command}': {e}"
        logger.error(error_message)
        # Check for common file not found error
        if "No such file or directory" in str(e):
             error_message = f"Aider command '{aider_command}' not found. Make sure Aider is installed and in the system PATH or configure 'aider_command' in config.yaml."
        return None, error_message
    except Exception as e:
        error_message = f"An unexpected error occurred while running Aider interactively: {e}"
        logger.exception(error_message) # Log full traceback
        return None, error_message
    finally:
        # Ensure the child process is closed if it's still alive
        if 'child' in locals() and isinstance(child, pexpect.spawn) and child.isalive():
            logger.warning("Aider process still alive at the end of interaction, closing.")
            child.close(force=True)
