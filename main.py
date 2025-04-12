import argparse
import logging
import os
import sys # Add sys import for exiting
import time # Import the time module
from pathlib import Path
import rich
from rich.console import Console
from rich.logging import RichHandler
import threading # For running UI server in background
import asyncio # For running async UI server
import anyio # For creating streams
import yaml # For loading config early
import http.server
import socketserver
from functools import partial

from src.harness import Harness
from src.ui_server import UIServer # Import UI Server

# Default configuration values (used if config file is missing or invalid)
DEFAULT_CONFIG = {
    "websocket_host": "localhost",
    "websocket_port": 9940, # Default WebSocket port
    "http_port": 9950, # Default HTTP port
    "enable_ui": False,
    # Set project_dir to the project root (parent of this file)
    "project_dir": str(Path(__file__).parent.resolve()),
    # Add other essential defaults if needed for early access
}

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
        "--aider-model",
        type=str,
        default=None,
        help="Specify the model for Aider to use (overrides config file).",
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
    parser.add_argument(
        "--ui-http-port",
        type=int,
        default=None, # Default comes from config
        help="HTTP port for serving the UI static files (overrides config).",
    )
 
 
    args = parser.parse_args()

    # Ensure work directory exists
    work_dir_path = Path(args.work_dir)
    work_dir_path.mkdir(parents=True, exist_ok=True)
    logger.info(f"Using working directory: {work_dir_path.resolve()}")

    # Determine the prompt source (command line or file)
    prompt_source_arg = None
    if args.prompt:
        prompt_source_arg = args.prompt
        logger.info("Using goal prompt from command line argument.")
    else:
        # Check if the goal prompt file exists before passing its path
        goal_file_path = Path(args.goal_prompt_file)
        if not goal_file_path.is_file():
            logger.error(f"Goal prompt file not found: {args.goal_prompt_file}")
            logger.error("Please create the goal prompt file or specify a valid path using --goal-prompt-file.")
            sys.exit(1) # Exit if the prompt file is essential and not found
        prompt_source_arg = args.goal_prompt_file # Pass the filename string
        logger.info(f"Using goal prompt file: {args.goal_prompt_file}")


    # Display banner
    console.print("\n[bold blue]Aider Autoloop Harness[/bold blue]")
    console.print("[italic]Self-Building Agent Framework[/italic]\n")

    # --- Load Config Early for UI ---
    config = DEFAULT_CONFIG.copy()
    config_path = Path(args.config_file)
    if config_path.is_file():
        try:
            with open(config_path, 'r') as f:
                loaded_config = yaml.safe_load(f)
            if isinstance(loaded_config, dict):
                config.update(loaded_config)
                logger.info(f"Loaded configuration from {config_path}")
            else:
                logger.warning(f"Config file {config_path} is not a valid dictionary. Using defaults.")
        except yaml.YAMLError as e:
            logger.error(f"Error parsing config file {config_path}: {e}. Using defaults.")
        except Exception as e:
            logger.error(f"Error reading config file {config_path}: {e}. Using defaults.")
    else:
        logger.warning(f"Config file {config_path} not found. Using defaults.")

    # Determine final UI settings (CLI args override config)
    ui_enabled = args.enable_ui or config.get("enable_ui", False)
    # WebSocket settings
    ws_host = args.ui_host or config.get("websocket_host", "localhost")
    ws_port = args.ui_port or config.get("websocket_port", 9940) # Use new default
    # HTTP settings
    http_host = args.ui_host or config.get("websocket_host", "localhost") # Usually same host
    # Default HTTP port comes from config/defaults, not derived from WS port anymore
    http_port = args.ui_http_port or config.get("http_port", 9950) # Use new default

    # --- Define HTTP Server Function ---
    def start_http_server(host: str, port: int, directory: Path):
        """Starts a simple HTTP server in the current thread."""
        handler_class = partial(http.server.SimpleHTTPRequestHandler, directory=str(directory))
        # Allow reusing the address immediately to prevent "Address already in use" errors on quick restarts
        socketserver.TCPServer.allow_reuse_address = True
        try:
            with socketserver.TCPServer((host, port), handler_class) as httpd:
                logger.info(f"HTTP server serving '{directory}' started on http://{host}:{port}")
                # Store httpd instance so it can be shut down
                global httpd_instance
                httpd_instance = httpd
                httpd.serve_forever() # This blocks until shutdown() is called
        except OSError as e:
            # Log specific error if port is in use
            if "Address already in use" in str(e):
                 logger.error(f"HTTP server failed: Port {port} is already in use.")
            else:
                 logger.error(f"Failed to start HTTP server on {host}:{port}: {e}")
            # Signal main thread about failure? For now, just log.
            httpd_instance = None # Ensure instance is None on failure
        except Exception as e:
            logger.error(f"HTTP server thread encountered an error: {e}", exc_info=True)
            httpd_instance = None
        finally:
            # This block runs *after* serve_forever() returns (i.e., after shutdown)
            logger.info(f"HTTP server on {host}:{port} has shut down.")

    # Create the communication stream for UI updates *before* initializing UIServer
    # Use infinite buffer to prevent blocking harness if UI server lags/crashes
    send_stream, receive_stream = None, None # Initialize to None
    try:
        send_stream, receive_stream = anyio.create_memory_object_stream(max_buffer_size=float('inf'))
        logger.info("Successfully created anyio memory object stream for UI.")
    except Exception as e:
        logger.error(f"Failed to create anyio memory object stream: {e}", exc_info=True)
        # Decide how to proceed. If UI is essential, maybe exit?
        # If UI is optional, we can continue but UI features will be disabled.
        # For now, log the error and set streams to None, allowing Harness init to handle it.
        send_stream, receive_stream = None, None
        # Potentially disable UI explicitly if creation fails?
        # ui_enabled = False # Consider this if stream is critical for UI

    # Create a directory for the UI if it doesn't exist
    ui_dir_path = Path(__file__).parent / "ui"
    ui_dir_path.mkdir(exist_ok=True)

    # --- Start UI Servers (if enabled) ---
    ui_server = None
    ws_server_thread = None
    http_server_thread = None
    httpd_instance = None # Global variable to hold the HTTP server instance for shutdown
    ui_dir_path = Path(__file__).parent / "ui"

    if ui_enabled:
        logger.info("UI is enabled. Starting WebSocket and HTTP servers...")

        # Create the UI Server instance *with* the stream
        ui_server = UIServer(host=ws_host, port=ws_port, receive_stream=receive_stream)

        # Start WebSocket Server
        def run_ws_server():
            try:
                # Ensure an event loop exists for this thread
                asyncio.run(ui_server.start())
            except Exception as e:
                logger.error(f"WebSocket server thread encountered an error: {e}", exc_info=True)

        ws_server_thread = threading.Thread(target=run_ws_server, daemon=True, name="WebSocketServerThread")
        ws_server_thread.start()
        logger.info(f"WebSocket server starting in background thread on ws://{ws_host}:{ws_port}")

        # Start HTTP Server
        http_server_thread = threading.Thread(
            target=start_http_server,
            args=(http_host, http_port, ui_dir_path),
            daemon=True,
            name="HttpServerThread"
        )
        http_server_thread.start()
        # Wait briefly to check if server started successfully (httpd_instance should be set)
        time.sleep(0.5)
        if http_server_thread.is_alive() and httpd_instance is None:
             logger.error("HTTP Server thread started but failed to bind to port. Check logs.")
             # Decide how to handle this - exit? Continue without HTTP?
             # For now, log the error and continue. UI might not be accessible via HTTP.
        elif not http_server_thread.is_alive():
             logger.error("HTTP Server thread failed to start. Check logs.")


# --- Council Planning Enforcement Function ---
import subprocess
import difflib
import datetime
import re

plan_path = Path("PLAN.md")
goal_prompt_path = Path("goal.prompt")
readme_path = Path("README.md")

def get_file_mtime(path):
    try:
        return path.stat().st_mtime
    except Exception:
        return 0

def read_file(path):
    try:
        return path.read_text(encoding="utf-8")
    except Exception:
        return ""

def council_planning_enforcement(iteration_number=None):
    """
    Enforce that the open source council convenes each round to collaboratively update PLAN.md,
    and only update goal.prompt for major shifts. All planning must respect README.md.
    All tests must pass to continue; after a few tries, the council can revert to a working commit.
    """
    plan_mtime_before = get_file_mtime(plan_path)
    goal_prompt_mtime_before = get_file_mtime(goal_prompt_path)

    console.print("\n[bold yellow]Council Planning Required[/bold yellow]")
    console.print(
        "[italic]At the end of each round, the open source council must collaboratively review and update [bold]PLAN.md[/bold] "
        "(very frequently) to reflect the current actionable plan, strategies, and next steps. "
        "Only update [bold]goal.prompt[/bold] if a significant change in overall direction is required (rare). "
        "All planning and actions must always respect the high-level goals and constraints in [bold]README.md[/bold].[/italic]"
    )
    console.print(
        "\n[bold]Please review and update PLAN.md now.[/bold] "
        "If a major shift in direction is needed, update goal.prompt as well."
    )
    console.print(
        "[italic]After updating, ensure all tests pass before proceeding. "
        "If tests fail after a few tries, the council should revert to a working commit using [bold]git revert[/bold].[/italic]"
    )

    plan_updated = False
    for attempt in range(2):  # One human chance, then auto-append
        old_plan = read_file(plan_path)
        console.print("\n[bold cyan]Waiting for PLAN.md to be updated with a new council round entry...[/bold cyan]")
        console.print("[italic]Please add a new checklist item or summary for this round in PLAN.md, then press Enter.[/italic]")
        input()
        new_plan = read_file(plan_path)
        if new_plan != old_plan:
            # Show a diff for transparency
            diff = list(difflib.unified_diff(
                old_plan.splitlines(), new_plan.splitlines(),
                fromfile="PLAN.md (before)", tofile="PLAN.md (after)", lineterm=""
            ))
            if diff:
                console.print("[bold green]PLAN.md updated. Diff:[/bold green]")
                for line in diff:
                    if line.startswith("+"):
                        console.print(f"[green]{line}[/green]")
                    elif line.startswith("-"):
                        console.print(f"[red]{line}[/red]")
                    else:
                        console.print(line)
            else:
                console.print("[yellow]PLAN.md changed, but no diff detected.[/yellow]")

            # Check for a new council round entry (e.g., a new checklist item or timestamp)
            has_actionable = ("- [ ]" in new_plan or "- [x]" in new_plan)
            has_summary = ("Summary of Last Round:" in new_plan)
            mentions_readme = ("README.md" in new_plan or "high-level goals" in new_plan.lower())
            if not has_summary:
                console.print("[bold yellow]Reminder:[/bold yellow] Please include a summary of the council's discussion and planning in PLAN.md for this round (add 'Summary of Last Round:').")
            if not mentions_readme:
                console.print("[bold yellow]Reminder:[/bold yellow] PLAN.md should always reference the high-level goals and constraints in README.md.")
                console.print("Please ensure your plan does not contradict the project's core direction.")
            if has_actionable and has_summary and mentions_readme:
                plan_updated = True
                break
            else:
                console.print("[bold red]PLAN.md does not appear to have a new actionable item, council summary, or reference to README.md/high-level goals. Please update accordingly.[/bold red]")
        else:
            console.print("[bold red]PLAN.md does not appear to have been updated. Please make changes before proceeding.[/bold red]")

    # If still not updated, auto-append a new council round entry
    if not plan_updated:
        now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        plan_content = read_file(plan_path)
        council_rounds = re.findall(r"Summary of Last Round:", plan_content)
        round_num = len(council_rounds) + 1
        new_entry = (
            f"\n---\n\n"
            f"### Council Round {round_num} ({now})\n"
            f"*   **Summary of Last Round:** [Auto-generated placeholder. Council did not update this round.]\n"
            f"*   **Blockers/Issues:** [None reported.]\n"
            f"*   **Next Steps/Tasks:**\n"
            f"    *   [ ] [Auto-generated] Review and update PLAN.md for next round.\n"
            f"*   **Reference:** This plan must always respect the high-level goals and constraints in README.md.\n"
        )
        with open(plan_path, "a", encoding="utf-8") as f:
            f.write(new_entry)
        console.print(f"[bold yellow]PLAN.md was not updated by a human. Auto-appended a new council round entry for round {round_num}.[/bold yellow]")
        # Show the new diff
        updated_plan = read_file(plan_path)
        diff = list(difflib.unified_diff(
            plan_content.splitlines(), updated_plan.splitlines(),
            fromfile="PLAN.md (before)", tofile="PLAN.md (after)", lineterm=""
        ))
        if diff:
            console.print("[bold green]Auto-update diff:[/bold green]")
            for line in diff:
                if line.startswith("+"):
                    console.print(f"[green]{line}[/green]")
                elif line.startswith("-"):
                    console.print(f"[red]{line}[/red]")
                else:
                    console.print(line)
        plan_updated = True

    # --- Check for major shift marker in PLAN.md to suggest goal.prompt update ---
    plan_content = read_file(plan_path)
    if "UPDATE_GOAL_PROMPT" in plan_content or "MAJOR_SHIFT" in plan_content:
        console.print("[bold magenta]A major shift was detected in PLAN.md. Please update goal.prompt accordingly.[/bold magenta]")
        console.print("[italic]Press Enter after updating goal.prompt.[/italic]")
        input()
    # Also, if goal.prompt was updated, require explicit confirmation
    goal_prompt_mtime_after = get_file_mtime(goal_prompt_path)
    if goal_prompt_mtime_after > goal_prompt_mtime_before:
        console.print("[bold magenta]goal.prompt was updated. Please confirm the new direction is correct.[/bold magenta]")
        console.print("[italic]Press Enter to continue.[/italic]")
        input()

    # --- Test Enforcement ---
    max_test_retries = 3
    for attempt in range(1, max_test_retries + 1):
        console.print(f"\n[bold]Running test suite (attempt {attempt}/{max_test_retries})...[/bold]")
        test_result = subprocess.run(["pytest", "-v"], cwd=".", capture_output=True, text=True)
        console.print(test_result.stdout)
        if test_result.returncode == 0:
            console.print("[bold green]All tests passed![/bold green]")
            break
        else:
            console.print(f"[bold red]Tests failed (attempt {attempt}).[/bold red]")
            if attempt < max_test_retries:
                console.print("[italic]Please fix the issues and update PLAN.md as needed, then press Enter to retry tests.[/italic]")
                input()
    else:
        console.print("[bold red]Tests failed after multiple attempts.[/bold red]")
        console.print("[bold yellow]The council should revert to a previous working commit using:[/bold yellow] [italic]git log[/italic] and [italic]git revert <commit>[/italic]")
        sys.exit(1)

    # Initialize and run the harness
    # (This block is intentionally left empty; main() will handle harness initialization and execution.)
    pass


if __name__ == "__main__":
    main()
