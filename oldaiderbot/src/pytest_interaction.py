import logging
import subprocess
import os
from typing import Tuple, List, Optional

def run_pytest(project_dir: str, test_args: Optional[List[str]] = None) -> Tuple[bool, str]:
    """
    Runs pytest in the specified directory and captures the output.

    Args:
        project_dir: The path to the directory containing the project and tests.
        test_args: Optional list of additional pytest arguments.

    Returns:
        A tuple containing:
        - bool: True if pytest passed (exit code 0), False otherwise.
        - str: The combined stdout and stderr from the pytest command.
    """
    # Base command
    command = ["pytest", "-v"]
    
    # Add any additional arguments
    if test_args:
        command.extend(test_args)
    
    logging.info(f"Running pytest command: {' '.join(command)} in {project_dir}")

    try:
        # Check if pytest is installed
        try:
            subprocess.run(
                ["pytest", "--version"],
                capture_output=True,
                text=True,
                check=False
            )
        except FileNotFoundError:
            error_message = "'pytest' command not found. Make sure pytest is installed in the environment."
            logging.error(error_message)
            return False, error_message
        
        # Set environment variables for pytest
        env = os.environ.copy()
        env["PYTHONPATH"] = project_dir + os.pathsep + env.get("PYTHONPATH", "")
        
        # Run pytest with a timeout
        process = subprocess.run(
            command,
            cwd=project_dir,
            capture_output=True,
            text=True,
            check=False, # Handle non-zero exit code manually
            timeout=300, # 5 minutes timeout
            env=env
        )

        # Combine stdout and stderr
        output = process.stdout + "\n" + process.stderr
        passed = process.returncode == 0

        # Log appropriate message based on result
        if passed:
            logging.info(f"Pytest finished successfully.")
        else:
            logging.warning(f"Pytest finished with failures (exit code {process.returncode}).")

        # Log truncated output at info level for visibility
        output_summary = output[:500] + "..." if len(output) > 500 else output
        logging.info(f"Pytest output summary:\n{output_summary}")
        
        # Log full output at debug level
        logging.debug(f"Full pytest output:\n{output}")

        return passed, output.strip()

    except subprocess.TimeoutExpired:
        error_message = "Pytest command timed out after 5 minutes."
        logging.error(error_message)
        return False, error_message
    except FileNotFoundError:
        # This should be caught by the earlier check, but just in case
        error_message = "'pytest' command not found. Make sure pytest is installed in the environment."
        logging.error(error_message)
        return False, error_message
    except Exception as e:
        error_message = f"An unexpected error occurred while running pytest: {e}"
        logging.exception(error_message) # Log full traceback
        return False, error_message
