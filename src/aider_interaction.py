import logging
import logging
import pexpect # Use pexpect for interactive control
import re
import shlex # Import shlex for quoting
import signal
import sys
import threading # Import threading
import time
from typing import List, Dict, Tuple, Optional, Any, Callable # Add Callable

# Import the LLM interaction function
from .llm_interaction import get_llm_response

# Configure logging for this module
logger = logging.getLogger(__name__)

# --- Constants for Interaction ---
# Regex patterns to detect common Aider prompts requiring a response
# Adjust these based on the exact prompts Aider uses
APPLY_PROMPT_PATTERN = r"Apply changes\? \[y/n/q/a/v\]"
PROCEED_PROMPT_PATTERN = r"Proceed\? \[y/n\]"
# Note: ADD_FILE_PROMPT_PATTERN removed as --yes should handle it.
# If issues arise, it might need to be re-added.
AIDER_PROMPT_PATTERNS = [
    APPLY_PROMPT_PATTERN,
    PROCEED_PROMPT_PATTERN,
    # Add other patterns if Aider has more interactive prompts that --yes doesn't cover
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
    interrupt_event: Optional[threading.Event] = None,
    output_callback: Optional[Callable[[str], None]] = None, # Add output callback
) -> Tuple[Optional[str], Optional[str]]:
    """
    Runs the Aider subprocess interactively using pexpect, allowing for interruption
    and streaming output via callback.

    Args:
        prompt: The user prompt for *this specific* Aider run.
        config: The harness configuration dictionary.
        history: The conversation history *prior* to this prompt (list of dicts).
        work_dir: The directory where Aider should run.
        interrupt_event: A threading.Event to signal interruption. If set, the function
                         will attempt to terminate Aider and return (None, "INTERRUPTED").
        output_callback: Optional callback function to receive chunks of Aider's stdout/stderr.

    Returns:
        A tuple containing:
        - The final extracted diff string (if successful).
        - An error message string (if an error occurred).
          Returns "INTERRUPTED" if stopped by interrupt_event.
    """
    # Get the base command string from config (e.g., "aider --model gemini")
    aider_command_str = config.get("aider_command", "aider")
    # Split the base command string into parts, respecting quotes
    command_args = shlex.split(aider_command_str)
    logger.info(f"Using base Aider command from config: {aider_command_str}")

    # Quote the prompt to handle spaces and special characters
    quoted_prompt = shlex.quote(prompt)

    # --- Append additional operational flags ---
    # Add --yes to automatically approve actions like applying changes
    command_args.append("--yes")
    logger.info("Appending --yes flag to Aider command.")

    # Add --auto-test to enable Aider's internal testing loop
    command_args.append("--auto-test")
    logger.info("Adding --auto-test flag to Aider command.")

    # Add --test-cmd, getting the command from config or using default
    test_command = config.get("aider_test_command", "pytest -v")
    quoted_test_command = shlex.quote(test_command)
    command_args.append(f"--test-cmd {quoted_test_command}")
    logger.info(f"Adding --test-cmd {quoted_test_command} flag to Aider command.")

    # Prompt will be sent via stdin, not as an argument
    # command_args.append(f"--message {quoted_prompt}") # Removed

    # Add other necessary aider flags from config if needed (ensure they don't conflict)
    # Example: command_args.extend(config.get("extra_aider_args", []))

    # Add the working directory itself as an argument for Aider to scan
    command_args.append(shlex.quote(work_dir))
    # Construct the full command string
    full_command = ' '.join(command_args)

    logger.info(f"Spawning Aider command (prompt via stdin): {' '.join(command_args)} in {work_dir}")
    # Log the prompt separately for clarity
    logger.debug(f"Aider initial prompt content (to be sent via stdin):\n{prompt}")
    full_output = "" # Accumulate all output from the session

    try:
        # Spawn the process
        child = pexpect.spawn(
            full_command,
            cwd=work_dir,
            encoding='utf-8',
            timeout=AIDER_TIMEOUT, # Overall timeout for the whole command
            logfile=None, # Disable direct logging, use callback instead
            # Use echo=False to prevent command input from being echoed back into the output buffer
            echo=False, # Ensure echo is False
        )

        # --- Send the prompt via stdin ---
        # Aider expects the prompt on stdin after startup
        logger.debug("Sending prompt to Aider via stdin...")
        child.sendline(prompt)
        # No need to explicitly wait for a prompt *from* Aider here,
        # as it should start processing the stdin prompt immediately.

        # Interaction loop - wait for output, completion, error, or interrupt
        # Use a shorter timeout in the loop to check the interrupt event frequently
        # and stream output promptly.
        loop_timeout = 0.5 # seconds (Reduced for responsiveness)

        while True:
            # --- Check for Interrupt Signal First ---
            if interrupt_event and interrupt_event.is_set():
                logger.warning("Interrupt signal received. Attempting to terminate Aider process.")
                full_output += "\n[Harness: Aider process interrupted by user signal]\n"
                # Send callback immediately to indicate interruption attempt
                if output_callback:
                    try:
                        output_callback("\n[Harness: Interrupt signal received. Terminating Aider...]\n")
                    except Exception as cb_err:
                        logger.error(f"Error in output_callback during interrupt: {cb_err}")

                try:
                    # --- Termination Sequence ---
                    # 1. Try graceful termination (SIGTERM) first. This allows Aider
                    #    to potentially clean up resources or save state if it handles the signal.
                    # 2. Wait for a short period.
                    # 3. If still alive, force termination (SIGKILL). SIGKILL cannot be caught
                    #    or ignored and guarantees the process stops.
                    if child.isalive():
                        logger.warning("Attempting graceful termination (SIGTERM) of Aider process.")
                        child.terminate(force=False) # Send SIGTERM
                        try:
                            # Wait briefly to see if it terminates gracefully
                            child.wait(timeout=2) # Wait up to 2 seconds
                            logger.info("Aider terminated gracefully after SIGTERM.")
                        except pexpect.exceptions.TIMEOUT:
                            # wait() timed out, process still alive
                            logger.warning("Aider did not terminate gracefully after SIGTERM. Sending SIGKILL.")
                            if child.isalive(): # Double-check before SIGKILL
                                child.terminate(force=True) # Send SIGKILL
                                time.sleep(0.1) # Short pause after kill
                            else:
                                logger.info("Aider terminated between SIGTERM check and SIGKILL attempt.")
                        except Exception as wait_exc:
                            # Handle potential errors during wait() itself
                            logger.error(f"Error during Aider SIGTERM wait: {wait_exc}. Attempting SIGKILL.")
                            if child.isalive():
                                child.terminate(force=True)
                                time.sleep(0.1)
                    else:
                        logger.info("Aider process already terminated before explicit signal.")
                except pexpect.exceptions.TIMEOUT:
                    # This can happen if wait() times out, even though we check isalive() before
                    logger.warning("Timeout waiting for Aider graceful shutdown. Sending SIGKILL.")
                    if child.isalive(): # Double check
                        child.terminate(force=True)
                        time.sleep(0.1)
                except Exception as term_exc:
                    logger.error(f"Error while trying to terminate Aider: {term_exc}")
                finally:
                    # Ensure the child is closed, regardless of termination success
                    if child.isalive(): # If still alive after all attempts, log error and force close pexpect
                        logger.error("Aider process still alive after termination attempts. Force closing pexpect connection.")
                        child.close(force=True) # Force close the pexpect object
                    elif not child.closed:
                        logger.info("Closing pexpect child process after termination.")
                        child.close() # Close the pexpect object itself (already terminated)
                return None, "INTERRUPTED" # Special return value

            try:
                # Wait for EOF or Timeout. We handle output streaming directly.
                # logger.debug(f"Waiting for Aider event (timeout={loop_timeout}s)...")
                patterns_to_expect = [
                    pexpect.EOF,                # Index 0: End of file (process finished)
                    pexpect.TIMEOUT             # Index 1: Timeout (means process is likely still running)
                ]
                # We don't explicitly expect known prompts anymore, as --yes handles them.
                # Output is captured via child.before on TIMEOUT or EOF.

                index = child.expect(patterns_to_expect, timeout=loop_timeout)

                # Capture output that came *before* the matched pattern (EOF or TIMEOUT)
                output_chunk = child.before
                if output_chunk:
                    # logger.debug(f"Raw output chunk received (len={len(output_chunk)}):\n>>>\n{output_chunk}\n<<<")
                    full_output += output_chunk
                    if output_callback:
                        try:
                            # Log before sending for debugging potential UI duplication
                            logger.debug(f"Sending chunk to UI callback (len={len(output_chunk)}): {output_chunk[:100].replace(chr(27), '[ESC]')}{'...' if len(output_chunk)>100 else ''}")
                            # Send the chunk to the callback for real-time UI updates
                            output_callback(output_chunk)
                            # Small sleep to allow UI to process the chunk - maybe remove if causing issues?
                            # time.sleep(0.01)
                        except Exception as cb_err:
                            # Log callback error but don't crash the interaction
                            logger.error(f"Error in output_callback: {cb_err}")

                # Process based on which pattern matched
                if index == 0: # EOF
                    logger.info("Aider process finished (EOF detected).")
                    # Any final output before EOF was captured in output_chunk above
                    child.close() # Close explicitly now that EOF is reached
                    break # Exit interaction loop, process finished normally
                elif index == 1: # Timeout
                    # This is the normal case when Aider is running/thinking.
                    # Output up to this point was captured in output_chunk.
                    # Loop continues to check interrupt and wait for more output/EOF.
                    # Add a small sleep to prevent tight-looping on timeout when idle
                    time.sleep(0.05) # 50ms sleep

            except pexpect.exceptions.ExceptionPexpect as e:
                 # Catch specific pexpect errors during the expect() call (other than Timeout/EOF)
                 logger.error(f"Pexpect error during Aider interaction: {e}")
                 logger.debug(f"Accumulated output before error:\n{full_output}")
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
             logger.info(f"Aider command finished successfully (Exit Code: {child.exitstatus}, Signal: {child.signalstatus}).")

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
             # Use the first part of the command (the executable) in the error message
             executable = command_args[0] if command_args else aider_command_str # Get executable or full string
             error_message = f"Aider command '{executable}' not found. Make sure Aider is installed and in the system PATH or configure 'aider_command' in config.yaml."
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
