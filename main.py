import argparse
import logging
import os
import time # Import the time module
from pathlib import Path
import rich
from rich.console import Console
from rich.logging import RichHandler
import threading # For running UI server in background
import asyncio # For running async UI server
import yaml # For loading config early
import http.server
import socketserver
from functools import partial
 
from src.harness import Harness
from src.ui_server import UIServer # Import UI Server

# Default configuration values (used if config file is missing or invalid)
DEFAULT_CONFIG = {
    "websocket_host": "localhost",
    "websocket_port": 8765,
    "http_port": 8676, # Default HTTP port
    "enable_ui": False,
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
    ws_port = args.ui_port or config.get("websocket_port", 8765)
    # HTTP settings
    http_host = args.ui_host or config.get("websocket_host", "localhost") # Usually same host
    http_port = args.ui_http_port or config.get("http_port", ws_port + 1) # Default to ws_port + 1

    # --- Define HTTP Server Function ---
    def start_http_server(host: str, port: int, directory: Path):
        """Starts a simple HTTP server in the current thread."""
        handler_class = partial(http.server.SimpleHTTPRequestHandler, directory=str(directory))
        try:
            with socketserver.TCPServer((host, port), handler_class) as httpd:
                logger.info(f"HTTP server serving '{directory}' started on http://{host}:{port}")
                httpd.serve_forever()
        except OSError as e:
            logger.error(f"Failed to start HTTP server on {host}:{port}: {e}")
        except Exception as e:
            logger.error(f"HTTP server thread encountered an error: {e}", exc_info=True)
        finally:
            logger.info(f"HTTP server on {host}:{port} stopped.")


    # --- Start UI Servers Early (if enabled) ---
    ui_server = None
    ws_server_thread = None # Renamed from ui_server_thread
    http_server = None # Added for HTTP server handle (though not used for shutdown here)
    http_server_thread = None # Added for HTTP server thread
    # Determine UI dir path relative to main.py's location
    ui_dir_path = Path(__file__).parent / "ui" 

    if ui_enabled:
        logger.info("UI is enabled. Starting WebSocket and HTTP servers...")
        
        # Start WebSocket Server
        ui_server = UIServer(host=ws_host, port=ws_port)
        def run_ws_server(): # Renamed from run_server
            try:
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
        # Note: We don't have a direct handle to the httpd object to call shutdown cleanly from here.
        # Daemon threads will exit when the main thread exits. For cleaner shutdown, 
        # start_http_server would need modification (e.g., using httpd.shutdown() via another thread/signal).

    # Initialize and run the harness
    try:
        # Create harness, passing determined UI settings
        harness = Harness(
            config_file=args.config_file, # Harness still loads its full config internally
            max_retries=args.max_retries,
            work_dir=work_dir_path,
            reset_state=args.reset_state,
            ollama_model=args.ollama_model,
            storage_type=args.storage_type,
            enable_council=not args.disable_council,
            enable_code_review=args.enable_code_review,
            # Pass the final determined UI settings to Harness constructor
            # These might override what Harness loads from its config again, which is fine.
            enable_ui=ui_enabled,
            websocket_host=ws_host, # Pass WS host/port
            websocket_port=ws_port,
            # http_port=http_port # Harness doesn't need the HTTP port directly
        )

        # Link the already running UI server instance to the harness
        if ui_server:
            harness.set_ui_server(ui_server)
            logger.info("Linked running UI server instance to Harness.")

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
        # Stop the WebSocket server if it was started
        if ui_server and ws_server_thread and ws_server_thread.is_alive():
            logger.info("Stopping WebSocket server...")
            ui_server.stop() # Signal the server loop to stop
            ws_server_thread.join(timeout=5) # Wait for thread to finish
            if ws_server_thread.is_alive():
                 logger.warning("WebSocket server thread did not stop cleanly.")
            else:
                 logger.info("WebSocket server stopped.") # Corrected log message
        
        # Log HTTP server thread status (it's a daemon, so it will exit, but we can check)
        if http_server_thread and http_server_thread.is_alive():
            logger.info("HTTP server thread is still running (expected for daemon thread).")
        elif http_server_thread:
            logger.info("HTTP server thread has stopped.")


if __name__ == "__main__":
    main()
