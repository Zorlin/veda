import argparse
import logging
import os
from pathlib import Path
import rich
from rich.console import Console
from rich.logging import RichHandler
import threading # For running UI server in background
import asyncio # For running async UI server

from src.harness import Harness
from src.ui_server import UIServer # Import UI Server

# Configure rich console
console = Console()

# Configure logging with rich
logging.basicConfig(
    level=logging.INFO,
    format="%(message)s",
    handlers=[RichHandler(rich_tracebacks=True, console=console)]
)

logger = logging.getLogger("aider_harness")


def main():
    """Main entry point for the Aider Autoloop Harness."""
    parser = argparse.ArgumentParser(
        description="Aider Autoloop Harness: Self-Building Agent Framework"
    )
    parser.add_argument(
        "prompt",
        nargs="?",
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
    parser.add_argument(
        "--reset-state",
        action="store_true",
        help="Ignore any saved state and start a fresh run.",
    )
    parser.add_argument(
        "--ollama-model",
        type=str,
        default=None,
        help="Specify the Ollama model to use (overrides config file).",
    )
    parser.add_argument(
        "--storage-type",
        type=str,
        choices=["sqlite", "json"],
        default="sqlite",
        help="Storage type for the ledger (sqlite or json).",
    )
    parser.add_argument(
        "--disable-council",
        action="store_true",
        help="Disable the VESPER.MIND council for evaluation.",
    )
    parser.add_argument(
        "--enable-code-review",
        action="store_true",
        help="Enable code review for successful iterations.",
    )
    parser.add_argument(
        "--enable-ui",
        action="store_true",
        help="Enable the WebSocket server for the Alpine.js/Tailwind UI (overrides config).",
    )
    parser.add_argument(
        "--ui-host",
        type=str,
        default=None, # Default comes from config
        help="WebSocket host for the UI (overrides config).",
    )
    parser.add_argument(
        "--ui-port",
        type=int,
        default=None, # Default comes from config
        help="WebSocket port for the UI (overrides config).",
    )


    args = parser.parse_args()

    # Ensure work directory exists
    work_dir_path = Path(args.work_dir)
    work_dir_path.mkdir(parents=True, exist_ok=True)
    logger.info(f"Using working directory: {work_dir_path.resolve()}")

    # Determine the prompt source (command line or file)
    if args.prompt:
        initial_goal_prompt = args.prompt
        logger.info("Using goal prompt from command line argument")
    else:
        # Read initial goal prompt from file
        try:
            with open(args.goal_prompt_file, "r") as f:
                initial_goal_prompt = f.read()
            logger.info(f"Loaded initial goal from: {args.goal_prompt_file}")
        except FileNotFoundError:
            logger.error(f"Goal prompt file not found: {args.goal_prompt_file}")
            # Use the default prompt as a fallback
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
            logger.warning("Using default goal prompt")
            # Create the default goal file
            default_goal_path = Path(args.goal_prompt_file)
            if not default_goal_path.exists():
                with open(default_goal_path, "w") as f:
                    f.write(initial_goal_prompt.strip())
                logger.info(f"Created default goal file: {default_goal_path}")


    # Display banner
    console.print("\n[bold blue]Aider Autoloop Harness[/bold blue]")
    console.print("[italic]Self-Building Agent Framework[/italic]\n")
    
    # Initialize and run the harness
    try:
        # Create harness with enhanced features
        harness = Harness(
            config_file=args.config_file,
            max_retries=args.max_retries,
            work_dir=work_dir_path,
            reset_state=args.reset_state,
            ollama_model=args.ollama_model,
            storage_type=args.storage_type,
            enable_council=not args.disable_council,
            enable_code_review=args.enable_code_review,
            # UI settings can be overridden by CLI args
            enable_ui=args.enable_ui or None, # Pass None if not set, Harness will use config
            websocket_host=args.ui_host,      # Pass None if not set
            websocket_port=args.ui_port       # Pass None if not set
            websocket_port=args.ui_port       # Pass None if not set
        )
        
        # Start WebSocket server if enabled
        ui_server_thread = None
        ui_server = None
        if harness.config.get("enable_ui"):
            ui_host = harness.config.get("websocket_host", "localhost")
            ui_port = harness.config.get("websocket_port", 8765)
            ui_server = UIServer(host=ui_host, port=ui_port)
            harness.set_ui_server(ui_server) # Link server to harness

            def run_server():
                asyncio.run(ui_server.start())

            ui_server_thread = threading.Thread(target=run_server, daemon=True)
            ui_server_thread.start()
            logger.info(f"UI WebSocket server starting in background thread on ws://{ui_host}:{ui_port}")
            # Give the server a moment to start
            time.sleep(1)
        
        # Run the harness and get results
        result = harness.run(initial_goal_prompt)
        
        # Display summary
        console.print("\n[bold green]Harness Run Complete[/bold green]")
        console.print(f"Run ID: {result['run_id']}")
        console.print(f"Iterations: {result['iterations']}")
        console.print(f"Converged: {'Yes' if result['converged'] else 'No'}")
        console.print(f"Final Status: {result['final_status']}")
        
        # Suggest viewing results
        console.print("\n[bold]To view detailed results:[/bold]")
        if args.storage_type == "sqlite":
            console.print(f"SQLite database: {work_dir_path}/harness_ledger.db")
        else:
            console.print(f"JSON state file: {work_dir_path}/harness_state.json")
        
        # Check for changelogs and reviews
        changelog_dir = work_dir_path / "changelogs"
        if changelog_dir.exists() and any(changelog_dir.iterdir()):
            console.print(f"Changelogs: {changelog_dir}")
        
        review_dir = work_dir_path / "reviews"
        if review_dir.exists() and any(review_dir.iterdir()):
            console.print(f"Code Reviews: {review_dir}")
            
    except Exception as e:
        logger.exception(f"Harness execution failed: {e}")
        console.print(f"\n[bold red]Error:[/bold red] {str(e)}")
    finally:
        # Stop the UI server if it was started
        if ui_server and ui_server_thread and ui_server_thread.is_alive():
            logger.info("Stopping UI WebSocket server...")
            ui_server.stop()
            ui_server_thread.join(timeout=5) # Wait for thread to finish
            if ui_server_thread.is_alive():
                 logger.warning("UI server thread did not stop cleanly.")
            else:
                 logger.info("UI WebSocket server stopped.")


if __name__ == "__main__":
    main()
