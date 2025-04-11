import argparse
import logging
import os
from pathlib import Path

from src.harness import Harness

# Configure logging
logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s"
)


def main():
    """Main entry point for the Aider Autoloop Harness."""
    parser = argparse.ArgumentParser(
        description="Aider Autoloop Harness: Self-Building Agent Framework"
    )
    parser.add_argument(
        "prompt",
        nargs="?", # Make the prompt optional
        default=None,
        help="The initial goal prompt for Aider (overrides --goal-prompt-file if provided).",
    )
    parser.add_argument(
        "--goal-prompt-file",
        type=str,
        default="goal.prompt",
        help="Path to the file containing the initial goal prompt (used if prompt argument is not provided).",
    )
    parser.add_argument(
        "--config-file",
        type=str,
        default="config.yaml",
        help="Path to the configuration file.",
    )
    parser.add_argument(
        "--max-retries",
        type=int,
        default=5,
        help="Maximum number of retry attempts.",
    )
    parser.add_argument(
        "--work-dir",
        type=str,
        default="harness_work_dir",
        help="Working directory for logs, state, and intermediate files.",
    )
    # Add more arguments as needed (e.g., specific Ollama model, Aider args)

    args = parser.parse_args()

    # Ensure work directory exists
    work_dir_path = Path(args.work_dir)
    work_dir_path.mkdir(parents=True, exist_ok=True)
    logging.info(f"Using working directory: {work_dir_path.resolve()}")

    # Read initial goal prompt
    try:
        with open(args.goal_prompt_file, "r") as f:
            initial_goal_prompt = f.read()
        logging.info(f"Loaded initial goal from: {args.goal_prompt_file}")
    except FileNotFoundError:
        logging.error(f"Goal prompt file not found: {args.goal_prompt_file}")
        # Use the default prompt from README as a fallback if file not found
        initial_goal_prompt = """
Your task is to build a Python-based test harness that:

1. Launches an Aider subprocess to apply a code or test change.
2. Runs pytest against the updated project.
3. Evaluates the outcome using a local LLM (via Ollama) that decides if the result was:
   - Successful
   - Retry-worthy with suggestions
   - A structural failure
4. Logs diffs, outcomes, and retry metadata in a stateful SQLite or JSON ledger.
5. Supports a prompt history chain so Aider can reason over its own history.
6. Continues looping until a 'converged' verdict is reached or max attempts.
7. Optionally allows another Aider process to act as a code reviewer.

You are allowed to modify files, install packages, and manage subprocesses.
This harness must be able to work on any project with a `pytest`-compatible test suite.
"""
        logging.warning("Using default goal prompt from README.md.")
        # Optionally create the default goal file
        default_goal_path = Path(args.goal_prompt_file)
        if not default_goal_path.exists():
             with open(default_goal_path, "w") as f:
                 f.write(initial_goal_prompt.strip())
             logging.info(f"Created default goal file: {default_goal_path}")


    # Initialize and run the harness
    try:
        harness = Harness(
            config_file=args.config_file,
            max_retries=args.max_retries,
            work_dir=work_dir_path,
        )
        harness.run(initial_goal_prompt)
    except Exception as e:
        logging.exception(f"Harness execution failed: {e}")
        # Consider more specific error handling


if __name__ == "__main__":
    main()
