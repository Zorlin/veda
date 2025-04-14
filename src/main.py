import argparse
import threading
import time
import sys
import logging
import webbrowser
import os
import json

# Allow running as "python src/main.py" from project root and finding other src modules
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

# --- Project Imports ---
try:
    from constants import OPENROUTER_API_KEY, OLLAMA_URL, VEDA_CHAT_MODEL
    from agent_manager import AgentManager
    from web_server import start_web_server, broadcast_agent_update # Import broadcast function
    from chat import chat_interface, run_readiness_chat
except ImportError as e:
    print(f"Error importing Veda components: {e}")
    print("Please ensure all source files (constants.py, agent_manager.py, web_server.py, chat.py) exist in the 'src' directory.")
    sys.exit(1)

# --- Setup Logging ---
# Configure root logger
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] [%(name)s] %(message)s",
    handlers=[
        logging.StreamHandler(sys.stdout) # Log to console
        # TODO: Add file handler later if needed
    ]
)
# Suppress overly verbose logs from libraries if necessary
# logging.getLogger("werkzeug").setLevel(logging.WARNING)
# logging.getLogger("socketio").setLevel(logging.WARNING)
# logging.getLogger("engineio").setLevel(logging.WARNING)

logger = logging.getLogger("veda.main")


# --- Global Agent Manager Instance ---
# Instantiated early to be available for all commands if needed
# We will still check for API key specifically for commands that need it.
agent_manager: AgentManager = AgentManager()

# --- Main Application Logic ---

def main():
    # agent_manager is already instantiated globally

    parser = argparse.ArgumentParser(
        description="Veda - Software development that doesn't sleep.",
        formatter_class=argparse.RawTextHelpFormatter # Preserve formatting in help
    )
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # --- 'start' command ---
    start_parser = subparsers.add_parser("start", help="Start Veda agent manager and web server.")
    start_parser.add_argument("--prompt", help="Initial project prompt. If omitted, Veda will chat to define the goal.")
    start_parser.add_argument("--host", default="0.0.0.0", help="Host for the web server (default: 0.0.0.0)")
    start_parser.add_argument("--port", type=int, default=9900, help="Port for the web server (default: 9900)")

    # --- 'set' command ---
    set_parser = subparsers.add_parser("set", help="Set configuration options (currently only 'instances').")
    set_parser.add_argument("option", choices=["instances"], help="Configuration option to set.")
    set_parser.add_argument("value", help="Value to set (e.g., 'auto' or a positive integer for instances).")

    # --- 'chat' command ---
    subparsers.add_parser("chat", help="Open an interactive chat session with Veda's coordinator.")

    # --- 'web' command ---
    subparsers.add_parser("web", help="Open the Veda web interface in your browser.")

    # --- 'status' command ---
    subparsers.add_parser("status", help="Show the status of running agents via the web API.")

    # --- 'stop' command ---
    # subparsers.add_parser("stop", help="Stop the Veda agent manager (if running as daemon - TBD).") # Future command

    args = parser.parse_args()

    # --- Command Handling ---

    # --- Check for OpenRouter API Key (Required for 'start' and potentially 'chat' if it triggers agents) ---
    if not OPENROUTER_API_KEY and args.command in ["start"]:
         logger.error("OPENROUTER_API_KEY environment variable is not set.")
         print("\nError: OPENROUTER_API_KEY environment variable is not set.")
         print("This key is required to run Aider agents.")
         print("Please set the environment variable and try again.")
         print("Example: export OPENROUTER_API_KEY=\"your-key-here\"")
         sys.exit(1)
    elif not OPENROUTER_API_KEY and args.command == "chat":
         logger.warning("OPENROUTER_API_KEY is not set. Chat interface will work, but Veda cannot start Aider agents.")
         print("\nWarning: OPENROUTER_API_KEY is not set. You can chat with Veda, but it won't be able to start development agents.")


    if args.command == "start":
        logger.info("Starting Veda...")
        # Agent Manager is already instantiated globally

        # Start the Web Server, passing the agent manager instance
        web_server_thread = start_web_server(agent_manager, host=args.host, port=args.port)
        # Give the server a moment to start up
        time.sleep(1.5)

        initial_prompt = args.prompt

        # If no prompt provided, run the readiness chat
        if not initial_prompt:
            if not sys.stdin.isatty():
                # Non-interactive environment (e.g., CI/CD, testing)
                logger.warning("Running in non-interactive mode without a prompt. Using default.")
                initial_prompt = "Default task: Analyze the current project structure and suggest improvements."
                print(f"Running non-interactively. Using default prompt: '{initial_prompt}'")
            else:
                # Interactive environment, run readiness chat
                try:
                    initial_prompt = run_readiness_chat()
                    if initial_prompt is None:
                        logger.info("User exited readiness chat. Shutting down.")
                        print("Setup cancelled by user.")
                        # Attempt graceful shutdown if possible (though agent manager hasn't started threads yet)
                        if agent_manager:
                            agent_manager.stop()
                        sys.exit(0)
                except Exception as e:
                    logger.error(f"Error during readiness chat: {e}", exc_info=True)
                    print(f"\nAn error occurred during the readiness chat: {e}")
                    sys.exit(1)

        # Start the Agent Manager's main loop and initial agents
        logger.info(f"Starting Agent Manager with initial prompt: {initial_prompt[:100]}...")
        print(f"\n[INFO] Starting AgentManager...")
        agent_manager.start(initial_prompt=initial_prompt)
        print(f"[INFO] AgentManager started. Monitoring agent activity...")

        # Start periodic broadcasting of agent status to the UI
        # We might want to trigger this more intelligently later (e.g., on status change)
        start_periodic_broadcast_thread = threading.Thread(
            target=start_periodic_broadcast_loop, args=(5,), daemon=True # Broadcast every 5 seconds
        )
        start_periodic_broadcast_thread.start()

        # Wait a few seconds and print agent status summary
        time.sleep(2)
        try:
            agents = agent_manager.get_active_agents_status()
            if not agents:
                print("[WARNING] No agents are currently running. Check logs for errors.")
            else:
                print(f"[INFO] {len(agents)} agent(s) running:")
                for agent in agents:
                    print(f"  - Role: {agent.get('role','?')}, Status: {agent.get('status','?')}, Model: {agent.get('model','?')}")
        except Exception as e:
            print(f"[ERROR] Could not retrieve agent status: {e}")

        print(f"\n🚀 Veda is running!")
        print(f"   Web UI: http://localhost:{args.port}")
        print(f"   Agent Manager Status: Running")
        print(f"   Initial Goal: {initial_prompt[:100]}...")
        print("\nUse 'veda status' to check agent activity.")
        print("Press Ctrl+C to stop Veda.")

        try:
            # Keep the main thread alive to allow background threads (web server, agent manager) to run.
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            logger.info("Ctrl+C received. Shutting down Veda...")
            print("\n🔌 Shutting down Veda...")
            if agent_manager:
                agent_manager.stop()
            # Web server thread is daemon, will exit automatically
            logger.info("Veda shutdown complete.")
            print("Shutdown complete.")
            # Force exit if threads are stuck (shouldn't be necessary with daemons)
            os._exit(0)

    elif args.command == "set":
        # Setting options requires the manager to be running, ideally via API call.
        # For now, this CLI command is less useful if Veda runs as a background process.
        # Let's make it print a message suggesting API/Web UI usage.
        logger.warning("Setting options via CLI is currently informational. Use Web UI or API when available.")
        print("Setting options via CLI is currently informational.", file=sys.stderr, flush=True)
        if args.option == "instances":
            if args.value == "auto":
                print("Agent instance management set to auto.", file=sys.stderr, flush=True)
            else:
                print(f"Agent instances set to {args.value}.", file=sys.stderr, flush=True)
            agent_manager.set_instances(args.value) # Modify the global instance directly
            # The log message is handled inside set_instances

    elif args.command == "chat":
        print("Welcome to Veda chat", file=sys.stderr, flush=True)
        chat_interface()

    elif args.command == "web":
        url = f"http://localhost:9900" # Use default port for now
        print(f"Attempting to open web interface at {url}...")
        # Check if server is likely running (basic check)
        server_seems_running = False
        try:
            import requests
            # Quick HEAD request to see if something responds
            requests.head(url, timeout=1)
            print("Server seems to be running.")
            server_seems_running = True
        except requests.exceptions.ConnectionError:
            print("Veda server doesn't seem to be running.")
            print("You can start it with: veda start")
        except requests.exceptions.Timeout:
             print("Web server is running but not responding quickly.")
             server_seems_running = True # Still try to open
        except ImportError:
            print("Cannot check server status: 'requests' library not installed.")
            # Assume it might be running and try opening anyway
            server_seems_running = True
        except Exception as e:
            print(f"Error checking server status: {e}")
            # Assume it might be running
            server_seems_running = True

        if server_seems_running:
            webbrowser.open(url)

    elif args.command == "status":
        url = f"http://localhost:9900/api/threads" # Use default port
        print(f"Fetching agent status from {url}...")
        try:
            import requests
            resp = requests.get(url, timeout=5)
            resp.raise_for_status()
            agents = resp.json()
            if not agents:
                print("\nNo active agents reported by the server.")
            else:
                print("\n--- Active Veda Agents ---")
                for agent in agents:
                    status_color = "\033[92m" if agent['status'] == 'running' else \
                                   "\033[94m" if 'handoff' in agent['status'] else \
                                   "\033[93m" if 'waiting' in agent['status'] else \
                                   "\033[91m" if 'fail' in agent['status'] else \
                                   "\033[0m" # Default color
                    end_color = "\033[0m"
                    print(f"- ID: {agent['id']:<4} Role: {agent['role']:<15} "
                          f"Status: {status_color}{agent['status']:<20}{end_color} "
                          f"Model: {agent.get('model', 'N/A')}")
                    # Optional: Show output preview
                    # print("  Output Preview:")
                    # for line in agent.get('output_preview', []):
                    #     print(f"    {line}")
                print("--------------------------")
        except ImportError:
            print("Cannot fetch status: 'requests' library not installed. Please install it: pip install requests")
        except requests.exceptions.ConnectionError:
            print("\nError: Could not connect to the Veda server.")
            print("Ensure Veda is running ('veda start') and accessible at http://localhost:9900.")
        except requests.exceptions.Timeout:
            print("\nError: Timed out connecting to the Veda server.")
        except requests.exceptions.RequestException as e:
            print(f"\nError fetching status: {e}")
        except Exception as e:
            logger.error(f"An unexpected error occurred during status check: {e}", exc_info=True)
            print(f"\nAn unexpected error occurred: {e}")

    else:
        # No command provided or invalid command
        help_message = "Veda - Software development that doesn't sleep."
        # Print to stderr for test compatibility (tests expect stderr for help)
        print(help_message, file=sys.stderr, flush=True)
        parser.print_help(sys.stderr)
        # Print examples directly, as shown in README.md
        print("\nExamples:", file=sys.stderr, flush=True)
        print("  veda start --prompt \"Create a flask app with a single route\"", file=sys.stderr, flush=True)
        print("  veda start                 # Start Veda and chat to define the goal", file=sys.stderr, flush=True)
        # print("  veda set instances 5")   # Deferring detailed 'set' examples
        # print("  veda set instances auto")
        print("  veda chat                  # Chat with the running Veda instance", file=sys.stderr, flush=True)
        print("  veda web                   # Open the web UI in a browser", file=sys.stderr, flush=True)
        print("  veda status                # Show the status of active agents", file=sys.stderr, flush=True)


# Helper function for periodic broadcast loop
def start_periodic_broadcast_loop(interval_seconds):
    """Target function for the broadcast thread."""
    while True:
        broadcast_agent_update()
        time.sleep(interval_seconds)

# REMOVE DUPLICATE main() DEFINITION AND LEGACY CLI LOGIC
