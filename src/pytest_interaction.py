import logging
import subprocess
from typing import Tuple

def run_pytest(project_dir: str) -> Tuple[bool, str]:
    """
    Runs pytest in the specified directory and captures the output.

    Args:
        project_dir: The path to the directory containing the project and tests.

    Returns:
        A tuple containing:
        - bool: True if pytest passed (exit code 0), False otherwise.
        - str: The combined stdout and stderr from the pytest command.
    """
    # Consider adding configuration for the pytest command (e.g., specific markers)
    command = ["pytest"]
    logging.info(f"Running pytest command: {' '.join(command)} in {project_dir}")

    try:
        process = subprocess.run(
            command,
            cwd=project_dir,
            capture_output=True,
            text=True,
            check=False, # Handle non-zero exit code manually
            timeout=180 # Add a timeout (e.g., 3 minutes)
        )

        output = process.stdout + "\n" + process.stderr # Combine stdout and stderr
        passed = process.returncode == 0

        if passed:
            logging.info(f"Pytest finished successfully.")
        else:
            logging.warning(f"Pytest finished with failures (exit code {process.returncode}).")

        logging.debug(f"Pytest output:\n{output}") # Log full output at debug level

        return passed, output.strip()

    except subprocess.TimeoutExpired:
        error_message = "Pytest command timed out."
        logging.error(error_message)
        return False, error_message
    except FileNotFoundError:
        # This usually means pytest is not installed or not in PATH
        error_message = "'pytest' command not found. Make sure pytest is installed in the environment."
        logging.error(error_message)
        return False, error_message
    except Exception as e:
        error_message = f"An unexpected error occurred while running pytest: {e}"
        logging.exception(error_message) # Log full traceback
        return False, error_message
