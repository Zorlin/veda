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
APPLY_PROMPT_PATTERN = r"Apply changes\? \[y/n/q/a/v\]"
PROCEED_PROMPT_PATTERN = r"Proceed\? \[y/n\]"
# Pattern for the "Add file to chat?" prompt
ADD_FILE_PROMPT_PATTERN = r"Add file .* to the chat\? \(Y\)es/\(N\)o/\(A\)ll/\(S\)kip all/\(D\)on't ask again"

AIDER_PROMPT_PATTERNS = [
    APPLY_PROMPT_PATTERN,
    PROCEED_PROMPT_PATTERN,
    ADD_FILE_PROMPT_PATTERN, # Handle add file prompt
    # Add other patterns if Aider has more interactive prompts
]
# Default timeout for waiting for Aider output
AIDER_TIMEOUT = 300 # seconds (5 minutes)

# --- Helper function removed as --yes flag makes it unnecessary ---

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
    ollama_model = config.get("ollama_model") # Get the model from harness config

    # Quote the prompt to handle spaces and special characters
    quoted_prompt = shlex.quote(prompt)

    # Base command arguments
    command_args = []

    # Explicitly configure Aider to use the gemini model
    command_args.append("--model gemini")
    logger.info("Configuring Aider to use model: gemini")

    # Add --yes to automatically approve actions
    command_args.append("--yes")
    logger.info("Adding --yes flag to Aider command.")

    # Add the message argument
    command_args.append(f"--message {quoted_prompt}")

    # Add other necessary aider flags from config if needed (ensure they don't conflict)
    # Example: command_args.extend(config.get("extra_aider_args", []))

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
                # Wait only for EOF or Timeout, as --yes handles prompts
                logger.debug(f"Waiting for Aider to finish (EOF or timeout={AIDER_TIMEOUT}s)...")
                index = child.expect([pexpect.EOF, pexpect.TIMEOUT], timeout=AIDER_TIMEOUT)

                # Accumulate output that came before the EOF/Timeout
                output_before = child.before
                if output_before:
                    full_output += output_before
                logger.debug(f"Output before EOF/Timeout:\n>>>\n{output_before}\n<<<")

                if index == 0: # EOF
                    logger.info("Aider process finished (EOF detected).")
                    child.close() # Close explicitly
                    break # Exit loop, process finished normally
                elif index == 1: # Timeout
                    logger.error(f"Timeout waiting for Aider output after {AIDER_TIMEOUT} seconds.")
                    # full_output already contains output before timeout
                    child.close(force=True)
                    return None, f"Timeout waiting for Aider after {AIDER_TIMEOUT}s"

            except pexpect.exceptions.ExceptionPexpect as e:
                 logger.error(f"Pexpect error during Aider interaction: {e}")
                 logger.debug(f"Output before error:\n{full_output}")
                 if child.isalive():
                     child.close(force=True)
                 return None, f"Pexpect error: {e}"

        # --- Process finished, analyze output ---
        # Ensure exitstatus/signalstatus are checked *after* potential close() call in EOF block
        if child.exitstatus != 0:
            # Check if closed gracefully or crashed
            if child.signalstatus is not None: # Check signalstatus first
                 error_message = f"Aider command terminated unexpectedly by signal: {child.signalstatus}.\nOutput:\n{full_output}"
            elif child.exitstatus is not None: # Then check exitstatus
                 error_message = f"Aider command failed with exit code {child.exitstatus}.\nOutput:\n{full_output}"
            else:
                 # This case should be less likely now after explicit close()
                 error_message = f"Aider command failed with unknown status (exit={child.exitstatus}, signal={child.signalstatus}).\nOutput:\n{full_output}"
            # Removed redundant else block here
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
