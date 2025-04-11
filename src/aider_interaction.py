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
# Pattern for the "Attempt to fix test errors?" prompt
FIX_TESTS_PROMPT_PATTERN = r"Attempt to fix test errors\? \(Y\)es/\(N\)o"

AIDER_PROMPT_PATTERNS = [
    APPLY_PROMPT_PATTERN,
    PROCEED_PROMPT_PATTERN,
    ADD_FILE_PROMPT_PATTERN, # Handle add file prompt
    FIX_TESTS_PROMPT_PATTERN, # Handle fix tests prompt
    # Add other patterns if Aider has more interactive prompts
]
# Default timeout for waiting for Aider output
AIDER_TIMEOUT = 600 # seconds (10 minutes)

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
        prompt: The user prompt for *this specific* Aider run.
        config: The harness configuration dictionary.
        history: The conversation history *prior* to this prompt (list of dicts).
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
    # DO NOT explicitly set --model here. Let Aider use its config or defaults.
    # The harness uses its configured model for *evaluation*, not for Aider's internal LLM.
    command_args = []

    # Add --yes to automatically approve actions like applying changes
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
            # logfile=None, # Set to sys.stdout to see Aider's full output during run
            logfile=sys.stdout, # Uncomment for debugging Aider interaction
        )

        # Interaction loop - primarily waiting for Add File or completion/error
        while True:
            try:
                # Wait for Add File prompt, EOF, or Timeout.
                # The --yes flag should handle other prompts like Apply changes?
                logger.debug(f"Waiting for Aider output (expecting Add File prompt, EOF, or timeout={AIDER_TIMEOUT}s)...")
                # Define patterns to expect
                patterns_to_expect = [
                    ADD_FILE_PROMPT_PATTERN,    # Index 0
                    FIX_TESTS_PROMPT_PATTERN,   # Index 1
                    pexpect.EOF,                # Index 2
                    pexpect.TIMEOUT             # Index 3
                ]
                index = child.expect(patterns_to_expect, timeout=AIDER_TIMEOUT)

                # Accumulate output that came *before* the matched pattern
                output_before = child.before
                if output_before:
                    full_output += output_before
                    # Log the chunk received before the match
                    # logger.debug(f"Output chunk before match:\n>>>\n{output_before}\n<<<")

                # Process based on which pattern matched
                if index == 0: # Matched ADD_FILE_PROMPT_PATTERN
                    matched_prompt = child.after # The matched prompt text
                    full_output += matched_prompt
                    logger.info(f"Detected 'Add file' prompt: '{matched_prompt.strip()}'. Automatically responding 'n'.")
                    child.sendline('n') # Send 'n' automatically
                    # Continue waiting for next output
                elif index == 1: # Matched FIX_TESTS_PROMPT_PATTERN
                    matched_prompt = child.after # The matched prompt text
                    full_output += matched_prompt
                    logger.info(f"Detected 'Attempt to fix test errors?' prompt: '{matched_prompt.strip()}'. Automatically responding 'n'.")
                    child.sendline('n') # Send 'n' automatically
                    # Continue waiting for next output
                elif index == 2: # EOF
                    logger.info("Aider process finished (EOF detected).")
                    # Output after the last match (if any) is in child.before upon EOF
                    output_before_eof = child.before
                    if output_before_eof:
                        full_output += output_before_eof
                        # logger.debug(f"Final output chunk before EOF:\n>>>\n{output_before_eof}\n<<<")
                    child.close() # Close explicitly now that EOF is reached
                    break # Exit interaction loop, process finished normally
                elif index == 3: # Timeout
                    logger.error(f"Timeout waiting for Aider output after {AIDER_TIMEOUT} seconds.")
                    # Output before timeout is already accumulated in full_output
                    child.close(force=True) # Force close on timeout
                    return None, f"Timeout waiting for Aider after {AIDER_TIMEOUT}s"

            except pexpect.exceptions.ExceptionPexpect as e:
                 # Catch specific pexpect errors during the expect() call
                 logger.error(f"Pexpect error during Aider interaction: {e}")
                 logger.debug(f"Output before error:\n{full_output}")
                 if child.isalive():
                     child.close(force=True)
                 return None, f"Pexpect error: {e}"

        # --- Process finished (EOF or error), analyze output ---
        logger.debug(f"Full Aider session output:\n---\n{full_output}\n---")

        # Check exit status *after* the loop finishes or breaks
        # Ensure child.close() was called before checking status
        if not child.closed:
             logger.warning("Child process was not closed before status check, closing now.")
             child.close() # Ensure it's closed

        # Check for abnormal termination first
        if child.signalstatus is not None:
            error_message = f"Aider command terminated unexpectedly by signal: {child.signalstatus}.\nOutput:\n{full_output}"
            logger.error(error_message)
            return None, error_message
        # Check for non-zero exit code (indicates Aider reported an error)
        elif child.exitstatus != 0:
            error_message = f"Aider command failed with exit code {child.exitstatus}.\nOutput:\n{full_output}"
            logger.error(error_message)
            return None, error_message
        # If exit status is 0 and signal is None, it finished "successfully"
        else:
             logger.info(f"Aider command finished successfully (exit code {child.exitstatus}).")

        # --- Diff Extraction Logic (applied to full_output from successful run) ---
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
            # If no diff block is found, check for explicit messages indicating no changes
            logger.warning("Could not find standard ```diff ... ``` block in Aider session output.")
            # Aider typically prints these messages when --yes is used and no changes are made
            no_changes_messages = [
                "No changes were applied.",
                "No changes needed.",
                "Applied edit.", # Sometimes appears with empty diff? Check context.
                # Add other potential messages if observed
            ]
            # Check if any known "no changes" message exists in the output
            if any(msg in full_output for msg in no_changes_messages):
                 logger.info("Aider output indicates no changes were applied or needed.")
                 return "", None # Success, explicitly no changes (empty diff string)
            else:
                 # If no diff and no explicit "no changes" message, it's ambiguous
                 logger.warning("No diff block found and no explicit 'no changes' message detected. Returning empty diff, but this might indicate an issue.")
                 # Consider if this should be an error or handled differently
                 return "", None # Treat as success with no changes for now, but log warning

        # If diff_output was extracted but is empty string after stripping
        if not diff_output:
             logger.info("Extracted diff block is empty. Indicating no changes.")
             return "", None # Success, no changes (empty diff string)

        logger.info(f"Successfully extracted diff (length: {len(diff_output)}).")
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
