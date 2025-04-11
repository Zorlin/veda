import logging
import subprocess
from typing import List, Dict, Tuple, Optional

def run_aider(
    prompt: str,
    config: Dict,
    history: List[Dict[str, str]],
    work_dir: str,
) -> Tuple[Optional[str], Optional[str]]:
    """
    Runs the Aider subprocess with the given prompt and history.

    Args:
        prompt: The user prompt for Aider.
        config: The harness configuration dictionary.
        history: The conversation history (not directly used by aider command yet,
                 but could be for more complex invocation).
        work_dir: The directory where Aider should run.

    Returns:
        A tuple containing:
        - The generated diff string (if successful).
        - An error message string (if an error occurred).
    """
    aider_command = config.get("aider_command", "aider")
    # Basic command structure. Might need refinement based on how aider accepts prompts/history.
    # Using --message assumes aider can take the prompt directly.
    # We might need to pipe the prompt or use a temporary file depending on aider's CLI.
    command = [
        aider_command,
        "--message", prompt,
        # Add other necessary aider flags from config if needed
        # e.g., "--model", config.get("aider_model", "gpt-4")
        # "--yes" might be useful to auto-apply changes if appropriate
    ]

    logging.info(f"Running Aider command: {' '.join(command)} in {work_dir}")

    try:
        # Execute the command in the specified working directory
        process = subprocess.run(
            command,
            cwd=work_dir,
            capture_output=True,
            text=True,
            check=False, # Don't raise exception on non-zero exit code, handle manually
            timeout=300 # Add a timeout (e.g., 5 minutes)
        )

        stdout = process.stdout
        stderr = process.stderr

        if process.returncode != 0:
            error_message = f"Aider command failed with exit code {process.returncode}.\nStderr:\n{stderr}\nStdout:\n{stdout}"
            logging.error(error_message)
            return None, error_message

        logging.info(f"Aider command finished successfully. Stdout:\n{stdout}")
        if stderr:
             logging.warning(f"Aider command produced stderr:\n{stderr}")

        # --- Diff Extraction Logic ---
        # This is crucial and depends heavily on Aider's output format.
        # Option 1: Assume Aider prints ONLY the diff to stdout on success.
        # diff_output = stdout.strip()

        # Option 2: Look for diff markers (```diff ... ```) in stdout.
        diff_start = stdout.find("```diff")
        diff_end = -1
        if diff_start != -1:
            # Find the end marker after the start marker
            diff_end = stdout.find("```", diff_start + 7) # Start search after ```diff\n

        if diff_start != -1 and diff_end != -1:
            diff_output = stdout[diff_start + 7 : diff_end].strip() # Extract content between markers
            logging.info("Extracted diff block from Aider output.")
        else:
            # Fallback or alternative strategy if no diff block found
            logging.warning("Could not find standard ```diff ... ``` block in Aider output. Using full stdout as potential diff.")
            # Consider if the *entire* stdout might be the diff, or if specific lines indicate changes.
            # This might need adjustment based on observing actual aider output.
            diff_output = stdout.strip() # Use full output as a fallback

        if not diff_output:
             logging.warning("Aider ran successfully but produced no diff/output.")
             # Decide if this is an error or just no changes needed
             # return None, "Aider ran successfully but produced no diff." # Option: Treat as error
             return "", None # Option: Treat as success with no changes

        return diff_output, None # Return extracted diff, no error

    except subprocess.TimeoutExpired:
        error_message = "Aider command timed out."
        logging.error(error_message)
        return None, error_message
    except FileNotFoundError:
        error_message = f"Aider command '{aider_command}' not found. Make sure Aider is installed and in the system PATH or configure 'aider_command' in config.yaml."
        logging.error(error_message)
        return None, error_message
    except Exception as e:
        error_message = f"An unexpected error occurred while running Aider: {e}"
        logging.exception(error_message) # Log full traceback
        return None, error_message
