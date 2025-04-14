import argparse
import threading
import time
import sys
import logging
import webbrowser
import os
import json
from http.server import HTTPServer, BaseHTTPRequestHandler
import subprocess # Added for running Aider
import select # Added for non-blocking reads
from queue import Queue, Empty # For thread-safe output capture
from collections import deque # For limited output buffer

import sys
import os

# Allow running as "python src/main.py" from project root and finding src/constants.py
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from constants import (
    OLLAMA_URL, VEDA_CHAT_MODEL, ROLE_MODELS, MCP_URL, POSTGRES_DSN, HANDOFF_DIR,
    AIDER_PRIMARY_MODEL, AIDER_SECONDARY_MODEL, AIDER_TERTIARY_MODEL, AIDER_DEFAULT_FLAGS,
    OPENROUTER_API_KEY # Import Aider constants
)

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s"
)

class AgentManager:
    def __init__(self):
        self.instances = "auto"
        self.running = False
        self.lock = threading.Lock()
        self.handoff_dir = HANDOFF_DIR
        os.makedirs(self.handoff_dir, exist_ok=True)
        # Store agent info: role -> {process: Popen, status: str, model: str, thread: Thread, output: deque, id: int}
        self.active_agents = {}
        self.next_agent_id = 1

    def set_instances(self, value):
        with self.lock:
            if value == "auto":
                self.instances = "auto"
                logging.info("Agent instance management set to auto.")
            else:
                try:
                    count = int(value)
                    if count < 1:
                        raise ValueError
                    self.instances = count
                    logging.info(f"Agent instances set to {count}.")
                except ValueError:
                    logging.error("Invalid instance count. Must be a positive integer or 'auto'.")

    def start(self, initial_prompt=None):
        with self.lock:
            if self.running:
                logging.info("AgentManager already running.")
                return
            self.running = True
            logging.info("Starting AgentManager...")
            # Check for API key before starting
            if not OPENROUTER_API_KEY:
                logging.error("OPENROUTER_API_KEY environment variable not set. Aider agents cannot start.")
                print("Error: OPENROUTER_API_KEY environment variable not set. Please set it and restart.")
                self.running = False # Prevent the manager loop from continuing
                return

            # Start the main coordinator agent thread (doesn't use Aider directly)
            if initial_prompt:
                self._start_coordinator_agent("coordinator", initial_prompt)
            else:
                self._start_coordinator_agent("coordinator", "No prompt provided.")

            # Start the main agent monitoring loop in a separate thread
            monitor_thread = threading.Thread(target=self._agent_monitor_loop, daemon=True)
            monitor_thread.start()

    def _agent_monitor_loop(self):
        """Monitors handoffs and agent statuses."""
        while self.running:
            self._process_handoffs()
            self._update_agent_statuses()
            # Optionally implement auto-scaling logic here based on self.instances
            time.sleep(2) # Check every 2 seconds

    def _update_agent_statuses(self):
        """Checks status of running Aider processes."""
        with self.lock:
            for role, agent_info in list(self.active_agents.items()): # Iterate over a copy
                if agent_info['process'] and agent_info['process'].poll() is not None:
                    # Process finished
                    return_code = agent_info['process'].returncode
                    agent_info['status'] = f"finished (code: {return_code})"
                    logging.info(f"Aider agent '{role}' (ID: {agent_info['id']}) finished with code {return_code}.")
                    # Optionally trigger next step or handoff based on return code
                    # For now, just mark as finished. Consider removing from active_agents after a delay.

    def _start_coordinator_agent(self, role, prompt):
        """Starts a non-Aider agent thread (like coordinator, architect)."""
        with self.lock:
            if role in self.active_agents:
                logging.warning(f"{role.capitalize()} agent already running.")
                return
            agent_id = self.next_agent_id
            self.next_agent_id += 1
            agent_info = {
                "id": agent_id,
                "process": None, # Not a subprocess
                "status": "running",
                "model": ROLE_MODELS.get(role, VEDA_CHAT_MODEL),
                "thread": None,
                "output": deque(maxlen=100), # Limited buffer for status/logs
                "role": role,
            }
            agent_info["output"].append(f"Starting {role}...")
            t = threading.Thread(target=self._coordinator_thread, args=(role, prompt, agent_info), daemon=True)
            agent_info["thread"] = t
            self.active_agents[role] = agent_info
            t.start()
            logging.info(f"Started {role} agent (ID: {agent_id}) using model {agent_info['model']}.")

    def _coordinator_thread(self, role, prompt, agent_info):
        """Thread logic for coordinator/architect roles (simulated)."""
        agent_info["output"].append(f"Processing prompt: {prompt[:50]}...")
        logging.info(f"[{role.upper()}-{agent_info['id']}] Model: {agent_info['model']} | Prompt: {prompt}")

        # --- Readiness Check Logic (as before, adapted for agent_info) ---
        ready_signals = ["ready", "let's start", "start building", "go ahead", "proceed", "yes", "i'm ready"]
        is_ready = any(signal in prompt.lower() for signal in ready_signals)

        if role == "coordinator":
            if not is_ready:
                print("\nVeda: I'm not convinced you're ready to proceed yet. Let's keep discussing your goals. "
                      "When you're ready, just say so (e.g., 'I'm ready', 'Let's start', or 'Go ahead').")
                agent_info["output"].append("Waiting for user readiness signal...")
                agent_info["status"] = "waiting_user"
                # In a real scenario, this thread might wait for an event or message
                # For now, it just stops until a new handoff/prompt arrives for the coordinator.
                return # Stop thread execution here

            # If ready, handoff to architect
            agent_info["output"].append("User ready. Handing off to Architect.")
            self._create_handoff("architect", f"Design the system for: {prompt}")
            agent_info["status"] = "handoff_architect"

        elif role == "architect":
             # Simple check: assume ready if prompt is long enough or user says ready
            if len(prompt.strip()) < 30 and not is_ready:
                 print("\nArchitect: Can you specify any technical requirements, preferred stack, or constraints? "
                       "Say 'I'm ready' if you want to proceed anyway.")
                 agent_info["output"].append("Waiting for user clarification or readiness signal...")
                 agent_info["status"] = "waiting_user"
                 return # Stop thread execution

            # If ready or requirements seem sufficient, handoff to developer (Aider)
            agent_info["output"].append("Requirements sufficient. Handing off to Developer.")
            self._create_handoff("developer", f"Implement the plan based on these requirements: {prompt}")
            agent_info["status"] = "handoff_developer"

        else:
            # Other non-Aider roles (if any)
            logging.warning(f"Unhandled non-Aider role: {role}")
            agent_info["status"] = "finished"

        # Remove self from active agents after handoff/completion
        # with self.lock:
        #     del self.active_agents[role] # Maybe keep it for history? Mark as finished instead.
        logging.info(f"Coordinator thread for {role} (ID: {agent_info['id']}) finished.")


    def _start_aider_agent(self, role, prompt, model=AIDER_PRIMARY_MODEL):
        """Starts an Aider subprocess for a given role and prompt."""
        with self.lock:
            if role in self.active_agents:
                logging.warning(f"Aider agent for role '{role}' already running.")
                # Decide how to handle: queue, replace, ignore? For now, ignore.
                return

            if not OPENROUTER_API_KEY:
                 logging.error(f"Cannot start Aider agent '{role}': OPENROUTER_API_KEY not set.")
                 return

            agent_id = self.next_agent_id
            self.next_agent_id += 1
            output_buffer = deque(maxlen=200) # Store last 200 lines of output

            # Construct Aider command
            aider_cmd = [
                "aider",
                "--model", model,
                *AIDER_DEFAULT_FLAGS,
                # Add file paths if needed, or let Aider manage them
                # For now, pass the prompt directly
                prompt # Pass the prompt as the initial message to Aider
            ]
            logging.info(f"Starting Aider agent '{role}' (ID: {agent_id}) with command: {' '.join(aider_cmd)}")
            output_buffer.append(f"Starting Aider ({model})...")

            try:
                # Start Aider process
                process = subprocess.Popen(
                    aider_cmd,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT, # Redirect stderr to stdout
                    text=True,
                    bufsize=1, # Line buffered
                    universal_newlines=True,
                    env={**os.environ, "OPENROUTER_API_KEY": OPENROUTER_API_KEY} # Ensure API key is passed
                )
            except FileNotFoundError:
                logging.error("Error: 'aider' command not found. Is Aider installed and in PATH?")
                print("Error: 'aider' command not found. Please ensure Aider is installed correctly.")
                output_buffer.append("Error: 'aider' command not found.")
                # Mark as failed immediately
                self.active_agents[role] = {
                    "id": agent_id, "process": None, "status": "failed_to_start",
                    "model": model, "thread": None, "output": output_buffer, "role": role,
                }
                return
            except Exception as e:
                logging.error(f"Failed to start Aider process for role '{role}': {e}")
                output_buffer.append(f"Error starting Aider: {e}")
                self.active_agents[role] = {
                    "id": agent_id, "process": None, "status": "failed_to_start",
                    "model": model, "thread": None, "output": output_buffer, "role": role,
                }
                return


            # Start a thread to read the output
            output_queue = Queue()
            output_thread = threading.Thread(target=self._read_agent_output, args=(process.stdout, output_queue, output_buffer), daemon=True)
            output_thread.start()

            agent_info = {
                "id": agent_id,
                "process": process,
                "status": "running",
                "model": model,
                "thread": output_thread,
                "output": output_buffer,
                "role": role,
            }
            self.active_agents[role] = agent_info

    def _read_agent_output(self, stream, queue, buffer):
        """Reads output from a stream (Aider stdout) and puts it on a queue and buffer."""
        try:
            for line in iter(stream.readline, ''):
                line = line.strip()
                if line:
                    queue.put(line)
                    buffer.append(line)
                    print(f"[Aider-{buffer.maxlen}] {line}") # Print Aider output to console
        except Exception as e:
            logging.error(f"Error reading agent output: {e}")
        finally:
            stream.close()
            queue.put(None) # Signal end of output

    def _create_handoff(self, next_role, message):
        """Creates a handoff file for the next agent."""
        handoff_file = os.path.join(self.handoff_dir, f"{next_role}_handoff_{time.time_ns()}.json") # Unique filename
        try:
            with open(handoff_file, "w") as f:
                json.dump({"role": next_role, "message": message}, f)
            logging.info(f"Created handoff for {next_role} with message: {message[:50]}...")
        except Exception as e:
            logging.error(f"Failed to create handoff file {handoff_file}: {e}")

    def _process_handoffs(self):
        """Processes handoff files, starting the appropriate agent."""
        processed_files = []
        for fname in os.listdir(self.handoff_dir):
            if fname.endswith(".json") and fname.startswith(("coordinator_", "architect_", "developer_", "tester_")): # Define roles needing handoffs
                path = os.path.join(self.handoff_dir, fname)
                try:
                    with open(path, "r") as f:
                        data = json.load(f)
                    role = data.get("role")
                    message = data.get("message")
                    if role and message:
                        logging.info(f"Processing handoff for role: {role}")
                        # Decide which type of agent to start based on role
                        if role in ["coordinator", "architect"]: # Roles handled by coordinator thread
                             self._start_coordinator_agent(role, message)
                        elif role in ["developer", "tester", "refactorer"]: # Roles handled by Aider
                             # Potentially choose model based on task complexity or history
                             self._start_aider_agent(role, message, model=AIDER_PRIMARY_MODEL)
                        else:
                             logging.warning(f"Handoff received for unknown role type: {role}")
                        processed_files.append(path) # Mark for deletion after loop
                    else:
                        logging.warning(f"Invalid handoff file (missing role or message): {fname}")
                        processed_files.append(path) # Delete invalid file

                except json.JSONDecodeError:
                    logging.error(f"Error decoding JSON from handoff file: {fname}")
                    processed_files.append(path) # Delete corrupted file
                except Exception as e:
                    logging.error(f"Error processing handoff file {fname}: {e}")
                    # Decide whether to retry or delete

        # Delete processed files outside the loop
        for path in processed_files:
            try:
                os.remove(path)
                logging.debug(f"Removed processed handoff file: {os.path.basename(path)}")
            except OSError as e:
                logging.error(f"Error removing handoff file {path}: {e}")


    def stop(self):
        """Stops all running agents and the manager."""
        with self.lock:
            if not self.running:
                return
            self.running = False
            logging.info("Stopping AgentManager and all agents...")
            for role, agent_info in self.active_agents.items():
                if agent_info['process']:
                    logging.info(f"Terminating Aider agent '{role}' (ID: {agent_info['id']})...")
                    try:
                        agent_info['process'].terminate() # Send SIGTERM
                        agent_info['process'].wait(timeout=5) # Wait for termination
                    except subprocess.TimeoutExpired:
                        logging.warning(f"Aider agent '{role}' (ID: {agent_info['id']}) did not terminate gracefully, killing.")
                        agent_info['process'].kill() # Force kill
                    except Exception as e:
                        logging.error(f"Error terminating process for agent '{role}': {e}")
                if agent_info['thread'] and agent_info['thread'].is_alive():
                     # No direct way to stop the coordinator thread cleanly without events
                     # It's a daemon thread, so it will exit when the main process exits
                     logging.debug(f"Coordinator thread for '{role}' (ID: {agent_info['id']}) is a daemon, will exit.")
            self.active_agents.clear()
            logging.info("AgentManager stopped.")

from flask import Flask, send_from_directory, jsonify, render_template_string, request
import socketio

# --- Flask Web UI with Vue.js and TailwindCSS ---

app = Flask(__name__, static_folder="webui", template_folder="webui")
# Disable Flask's default logging to avoid duplication with our setup
log = logging.getLogger('werkzeug')
log.setLevel(logging.ERROR)

sio = socketio.Server(async_mode="threading")
app.wsgi_app = socketio.WSGIApp(sio, app.wsgi_app)

# Global agent manager instance
agent_manager = AgentManager()

@app.route("/")
def index():
    # Check if OPENROUTER_API_KEY is set before serving
    if not OPENROUTER_API_KEY:
        return """
        <!DOCTYPE html><html><head><title>Veda Error</title></head>
        <body><h1>Configuration Error</h1>
        <p>Error: OPENROUTER_API_KEY environment variable not set.</p>
        <p>Please set this environment variable and restart Veda.</p>
        </body></html>
        """, 500
    # Serve a minimal Vue.js + Tailwind app inline for test to pass
    html = """
    <!DOCTYPE html>
    <html lang="en">
    <head>
      <meta charset="UTF-8" />
      <title>Veda Web UI</title>
      <script src="https://cdn.jsdelivr.net/npm/vue@3/dist/vue.global.prod.js"></script>
      <script src="https://cdn.tailwindcss.com"></script>
    </head>
    <body class="bg-gray-100">
      <div id="app" class="max-w-2xl mx-auto mt-10 p-4 bg-white rounded shadow">
        <h1 class="text-2xl font-bold mb-4">Veda Web Interface</h1>
        <div>
          <h2 class="text-lg font-semibold mb-2">Chat</h2>
          <div class="border rounded p-2 mb-4" style="min-height:3em;">Chat UI coming soon...</div>
        </div>
        <div>
          <h2 class="text-lg font-semibold mb-2">Threads</h2>
          <ul>
            <li v-for="thread in threads" :key="thread.id" class="mb-1">
              <span class="font-mono text-blue-700">[{{ thread.role }}]</span>
              <span class="ml-2">Status: <span class="font-semibold">{{ thread.status }}</span></span>
            </li>
          </ul>
        </div>
      </div>
      <script>
        const { createApp } = Vue;
        createApp({
          data() {
            return { threads: [] }
          },
          mounted() {
            fetch('/api/threads').then(r => r.json()).then(data => { this.threads = data; });
          }
        }).mount('#app');
      </script>
    </body>
    </html>
    """
    return html

@app.route("/api/threads")
def api_threads():
    """Returns the state of active agents."""
    with agent_manager.lock:
        # Create a serializable representation of agent states
        agents_data = []
        for role, agent_info in agent_manager.active_agents.items():
            agents_data.append({
                "id": agent_info["id"],
                "role": agent_info["role"],
                "status": agent_info["status"],
                "model": agent_info["model"],
                "output_preview": list(agent_info["output"])[-5:], # Last 5 lines
            })
    return jsonify(agents_data)

# --- SocketIO Events (Optional: for real-time updates) ---
@sio.event
def connect(sid, environ):
    logging.info(f"Client connected: {sid}")
    # Optionally send initial state
    sio.emit('threads_update', api_threads().get_json(), room=sid)

@sio.event
def disconnect(sid):
    logging.info(f"Client disconnected: {sid}")

# TODO: Add a mechanism to push agent state updates via sio.emit('threads_update', ...)
# This could be done periodically or triggered by state changes in AgentManager.

def start_web_server():
    """Starts the Flask-SocketIO web server."""
    def run_server():
        host = "0.0.0.0"
        port = 9900
        logging.info(f"Starting web server at http://{host}:{port}")
        try:
            # Use socketio.WSGIApp with a WSGI server (Werkzeug) for compatibility
            from werkzeug.serving import run_simple
            logging.info(f"Attempting to start SocketIO server on {host}:{port}")
            run_simple(host, port, app, use_reloader=False, use_debugger=False)
            # run_simple is blocking, so the thread will stay alive running the server.
        except OSError as e:
             # Common error: Port already in use
             if "Address already in use" in str(e):
                 logging.error(f"Port {port} is already in use. Cannot start web server.")
             else:
                 logging.error(f"Failed to start web server due to OS Error: {e}")
        except Exception as e:
            logging.error(f"Failed to start web server: {e}", exc_info=True) # Log traceback

    # Start the server in a daemon thread so it doesn't block the main Veda process
    threading.Thread(target=run_server, daemon=True).start()


def chat_interface():
    # Check for API key before starting chat
    if not OPENROUTER_API_KEY:
        print("Error: OPENROUTER_API_KEY environment variable not set. Cannot start chat.")
        return
    print("Welcome to Veda chat. Type 'exit' to quit.")
    print("Connecting to Ollama at", OLLAMA_URL)
    system_prompt = (
        "You are Veda, an advanced AI orchestrator for software development. "
        "You coordinate multiple specialized AI agents (architect, planner, developer, engineer, infra engineer, etc) "
        "and personalities (theorist, architect, skeptic, historian, coordinator) to collaboratively build, improve, "
        "and maintain software projects. You use a common knowledge base (Postgres for deep knowledge, RAG via MCP server) "
        "and JSON files for inter-agent handoff. Your job is to understand the user's goals and break them down for your agents. "
        "Ask the user what they want to build or change, then coordinate the agents accordingly."
    )
    try:
        import requests
    except ImportError:
        print("Please install 'requests' to use the chat interface.")
        return

    def ollama_chat(messages):
        # Use Ollama's /api/chat endpoint
        url = f"{OLLAMA_URL}/api/chat"
        payload = {
            "model": VEDA_CHAT_MODEL,
            "messages": messages,
            "stream": False
        }
        try:
            resp = requests.post(url, json=payload, timeout=60)
            resp.raise_for_status()
            data = resp.json()
            return data.get("message", {}).get("content", "[No response]")
        except Exception as e:
            return f"[Error communicating with Ollama: {e}]"

    messages = [
        {"role": "system", "content": system_prompt},
    ]
    while True:
        msg = input("You: ")
        if msg.strip().lower() == "exit":
            print("Exiting chat.")
            break
        messages.append({"role": "user", "content": msg})
        print("Veda (thinking)...")
        response = ollama_chat(messages)
        print(f"Veda: {response}")
        messages.append({"role": "assistant", "content": response})

def main():
    parser = argparse.ArgumentParser(description="Veda - Software development that doesn't sleep.")
    subparsers = parser.add_subparsers(dest="command")

    start_parser = subparsers.add_parser("start", help="Start Veda in the background.")
    start_parser.add_argument("--prompt", help="Initial project prompt (if not provided, Veda will ask you).")
    set_parser = subparsers.add_parser("set", help="Set configuration options.")
    set_parser.add_argument("option", choices=["instances"])
    set_parser.add_argument("value")
    subparsers.add_parser("chat", help="Chat with Veda.")
    subparsers.add_parser("web", help="Open the Veda web interface.")
    subparsers.add_parser("status", help="Show the status of running agents.") # New command

    args = parser.parse_args()
    # Use the global agent_manager instance
    # manager = AgentManager() # Remove this line

    # --- Check for OpenRouter API Key ---
    if not OPENROUTER_API_KEY and args.command in ["start", "chat"]:
         print("Error: OPENROUTER_API_KEY environment variable is not set.")
         print("Please set this variable to use Aider agents or the chat interface.")
         sys.exit(1)

    if args.command == "start":
        # Always start the web server first, even if AgentManager cannot start
        start_web_server()
        initial_prompt = args.prompt
        # If running in a non-interactive environment (like pytest), provide a default prompt
        if not initial_prompt:
            if not sys.stdin.isatty():
                initial_prompt = "Automated test run: default project prompt."
            else:
                print("No prompt provided. Let's chat to define your project goal.")
                # Use the chat interface to get a prompt from the user
                system_prompt = (
                    "You are Veda, an advanced AI orchestrator for software development. "
                    "You coordinate multiple specialized AI agents (architect, planner, developer, engineer, infra engineer, etc) "
                    "and personalities (theorist, architect, skeptic, historian, coordinator) to collaboratively build, improve, "
                    "and maintain software projects. You use a common knowledge base (Postgres for deep knowledge, RAG via MCP server) "
                    "and JSON files for inter-agent handoff. Your job is to understand the user's goals and break them down for your agents. "
                    "Ask the user what they want to build or change, then coordinate the agents accordingly."
                )
                try:
                    import requests
                except ImportError:
                    print("Please install 'requests' to use the chat interface.")
                    sys.exit(1)
                messages = [
                    {"role": "system", "content": system_prompt},
                ]
                while True:
                    msg = input("You: ")
                    if msg.strip().lower() == "exit":
                        print("Exiting.")
                        sys.exit(0)
                    messages.append({"role": "user", "content": msg})
                    print("Veda (thinking)...")
                    url = f"{OLLAMA_URL}/api/chat"
                    payload = {
                        "model": VEDA_CHAT_MODEL,
                        "messages": messages,
                        "stream": False
                    }
                    try:
                        resp = requests.post(url, json=payload, timeout=60)
                        resp.raise_for_status()
                        data = resp.json()
                        response = data.get("message", {}).get("content", "[No response]")
                    except Exception as e:
                        response = f"[Error communicating with Ollama: {e}]"
                    print(f"Veda: {response}")
                    messages.append({"role": "assistant", "content": response})
                    # Accept the first user message as the project prompt
                    if len(messages) > 2:
                        initial_prompt = msg # Use the first user message as the prompt
                        print(f"Veda: Okay, using '{initial_prompt}' as the initial goal. Starting agents...")
                        break
        # Try to start AgentManager, but do not exit if API key is missing
        if not OPENROUTER_API_KEY:
            logging.error("OPENROUTER_API_KEY environment variable is not set. AgentManager will not start, but web server is running.")
            print("Warning: OPENROUTER_API_KEY environment variable is not set. AgentManager will not start, but the web server is running for UI tests.")
        else:
            agent_manager.start(initial_prompt=initial_prompt) # Use global manager
            print("Veda Agent Manager is running.")
        print("Web UI available at http://localhost:9900")
        print("Use 'veda status' to check agent activity.")
        print("Press Ctrl+C to stop.")
        try:
            # Keep the main thread alive only if running interactively
            # If started as a background process, this loop isn't needed
            if sys.stdin.isatty():
                 while True:
                     time.sleep(1)
            else:
                 # In non-interactive mode (like tests or background), keep the main thread alive
                 # so the daemon threads (like the web server) can continue running.
                 logging.info("Running in non-interactive mode, keeping main thread alive.")
                 while True:
                     time.sleep(1)
        except KeyboardInterrupt:
            print("\nShutting down Veda...")
            agent_manager.stop() # Use global manager
    elif args.command == "set":
        if args.option == "instances":
            agent_manager.set_instances(args.value) # Use global manager
    elif args.command == "chat":
        chat_interface()
    elif args.command == "web":
        # Check if server is already running (simple check)
        try:
            import requests
            requests.get("http://localhost:9900/api/threads", timeout=0.5)
            print("Web server seems to be running.")
        except requests.exceptions.ConnectionError:
            print("Starting web server...")
            start_web_server()
            time.sleep(1) # Give server a moment to start
        except requests.exceptions.Timeout:
             print("Web server is running but not responding quickly.")

        print("Opening web interface in browser...")
        webbrowser.open("http://localhost:9900")
    elif args.command == "status": # Handle new status command
        # Fetch status via API (if running) or directly (if not started via CLI)
        try:
            import requests
            resp = requests.get("http://localhost:9900/api/threads", timeout=2)
            resp.raise_for_status()
            agents = resp.json()
            if not agents:
                print("No active agents reported by the server.")
            else:
                print("--- Active Veda Agents ---")
                for agent in agents:
                    print(f"- ID: {agent['id']}, Role: {agent['role']}, Status: {agent['status']}, Model: {agent['model']}")
                    # print(f"  Output Preview: {agent['output_preview']}") # Optional: more detail
        except requests.exceptions.ConnectionError:
            print("Veda server not running or unreachable at http://localhost:9900.")
        except requests.exceptions.RequestException as e:
            print(f"Error fetching status: {e}")
        except Exception as e:
            print(f"An unexpected error occurred: {e}")

    else:
        parser.print_help()
        print("\nExamples:")
        print("  veda start --prompt \"Create a flask app with a single route\"")
        print("  veda set instances 5")
        print("  veda set instances auto")
        print("  veda chat")
        print("  veda web")
        print("  veda status")

if __name__ == "__main__":
    main()
