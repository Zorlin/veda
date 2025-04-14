import threading
import time
import sys
import logging
import os
import json
import subprocess # Added for running Aider
import select # Added for non-blocking reads
from queue import Queue, Empty # For thread-safe output capture
from collections import deque # For limited output buffer

# Allow finding constants.py when run from project root
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

# Maximum number of lines to keep in the output buffer for each agent
AGENT_OUTPUT_BUFFER_SIZE = 200

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
                print("Agent instance management set to auto.", file=sys.stderr, flush=True)
                print("Agent instance management set to auto.", file=sys.stdout, flush=True)
            else:
                try:
                    count = int(value)
                    if count < 1:
                        raise ValueError
                    self.instances = count
                    logging.info(f"Agent instances set to {count}.")
                    print(f"Agent instances set to {count}.", file=sys.stderr, flush=True)
                    print(f"Agent instances set to {count}.", file=sys.stdout, flush=True)
                except ValueError:
                    logging.error("Invalid instance count. Must be a positive integer or 'auto'.")
                    print("Invalid instance count. Must be a positive integer or 'auto'.", file=sys.stderr, flush=True)
                    print("Invalid instance count. Must be a positive integer or 'auto'.", file=sys.stdout, flush=True)

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

            # Always start at least one aider agent for the current project directory
            # Let's rename the default aider agent role to 'developer' to align with RULES.md
            if "developer" not in self.active_agents:
                project_dir = os.getcwd()
                prompt = initial_prompt or f"Improve and maintain this codebase: {os.path.basename(project_dir)}"
                self._start_aider_agent("developer", prompt) # Changed role to 'developer'

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
            # Use the specific thread logic based on role
            if role == "coordinator":
                target_func = self._coordinator_thread
            elif role == "architect":
                target_func = self._architect_thread
            # Add other non-Aider roles here if needed
            else:
                logging.error(f"Attempted to start unknown non-Aider role: {role}")
                return

            t = threading.Thread(target=target_func, args=(prompt, agent_info), daemon=True)
            agent_info["thread"] = t
            self.active_agents[role] = agent_info
            t.start()
            logging.info(f"Started {role} agent (ID: {agent_id}) using model {agent_info['model']}.")

    def _coordinator_thread(self, prompt, agent_info):
        """Thread logic for the coordinator role."""
        role = agent_info["role"]
        agent_info["output"].append(f"Processing prompt: {prompt[:50]}...")
        logging.info(f"[{role.upper()}-{agent_info['id']}] Model: {agent_info['model']} | Prompt: {prompt}")

        # --- Readiness Check Logic (Simulated - LLM integration needed later) ---
        # This part should eventually use an LLM call based on RULES.md
        # For now, we keep the simple keyword check for basic flow.
        ready_signals = ["ready", "let's start", "start building", "go ahead", "proceed", "yes", "i'm ready"]
        is_ready = any(signal in prompt.lower() for signal in ready_signals)

        if not is_ready:
            print("\nVeda (Coordinator): I need to ensure we're aligned before starting. "
                  "Let's discuss your goals further. When you're ready, just say so "
                  "(e.g., 'I'm ready', 'Let's start').")
            agent_info["output"].append("Waiting for user readiness signal...")
            agent_info["status"] = "waiting_user"
            # In a real implementation, this thread would wait for an event or new input.
            # For now, it just finishes, expecting a new prompt/handoff later.
            logging.info(f"Coordinator thread (ID: {agent_info['id']}) waiting for user readiness.")
            return # Stop thread execution here until readiness confirmed

        # If ready, handoff to architect
        agent_info["output"].append("User ready. Handing off to Architect.")
        self._create_handoff("architect", f"Design the system for: {prompt}")
        agent_info["status"] = "handoff_architect"
        logging.info(f"Coordinator thread (ID: {agent_info['id']}) finished, handed off to Architect.")
        # Consider marking as 'finished' or removing after handoff? Let's mark finished.
        # agent_info["status"] = "finished" # Or keep 'handoff_architect'? Let's keep it.

    def _architect_thread(self, prompt, agent_info):
        """Thread logic for the architect role."""
        role = agent_info["role"]
        agent_info["output"].append(f"Designing based on: {prompt[:50]}...")
        logging.info(f"[{role.upper()}-{agent_info['id']}] Model: {agent_info['model']} | Requirements: {prompt}")

        # --- Requirements Check Logic (Simulated - LLM integration needed later) ---
        # This should also use LLM reasoning based on RULES.md.
        # Simple check for now.
        ready_signals = ["ready", "let's start", "start building", "go ahead", "proceed", "yes", "i'm ready"]
        is_ready_signal = any(signal in prompt.lower() for signal in ready_signals)
        sufficient_detail = len(prompt.strip()) >= 30 # Arbitrary length check

        if not sufficient_detail and not is_ready_signal:
             print("\nArchitect: The requirements seem a bit brief. Can you specify any technical details, "
                   "preferred stack, or constraints? Say 'Proceed anyway' or 'I'm ready' if you want to continue.")
             agent_info["output"].append("Waiting for user clarification or readiness signal...")
             agent_info["status"] = "waiting_user"
             logging.info(f"Architect thread (ID: {agent_info['id']}) waiting for user clarification.")
             return # Stop thread execution

        # If ready or requirements seem sufficient, handoff to developer (Aider)
        agent_info["output"].append("Requirements sufficient. Handing off to Developer.")
        # Pass the architect's plan (simulated as the original prompt for now)
        plan = f"Implement the system based on these requirements: {prompt}"
        self._create_handoff("developer", plan)
        agent_info["status"] = "handoff_developer"
        logging.info(f"Architect thread (ID: {agent_info['id']}) finished, handed off to Developer.")
        # agent_info["status"] = "finished" # Or keep 'handoff_developer'?

    def _start_aider_agent(self, role, prompt, model=None):
        """Starts an Aider subprocess for a given role and prompt."""
        with self.lock:
            if role in self.active_agents:
                logging.warning(f"Aider agent for role '{role}' already running.")
                # TODO: Decide how to handle: queue, replace, ignore? For now, ignore.
                return

            if not OPENROUTER_API_KEY:
                 logging.error(f"Cannot start Aider agent '{role}': OPENROUTER_API_KEY not set.")
                 return

            # Determine the model to use
            if model is None:
                # Basic logic: use primary for developer, secondary for others? Refine later.
                if role == "developer":
                    model = AIDER_PRIMARY_MODEL
                elif role == "refactorer":
                    model = AIDER_SECONDARY_MODEL
                else: # Default to primary if role not specified
                    model = AIDER_PRIMARY_MODEL
                logging.info(f"No model specified for role '{role}', defaulting to {model}")

            agent_id = self.next_agent_id
            self.next_agent_id += 1
            output_buffer = deque(maxlen=AGENT_OUTPUT_BUFFER_SIZE) # Use constant

            # Construct Aider command
            aider_cmd = [
                "aider",
                "--model", model,
                *AIDER_DEFAULT_FLAGS,
                # Add file paths if needed, or let Aider manage them
                # Pass the prompt directly as the initial message
                prompt
            ]
            logging.info(f"Starting Aider agent '{role}' (ID: {agent_id}) with command: {' '.join(aider_cmd)}")
            output_buffer.append(f"Starting Aider ({model})...")

            try:
                # Start Aider process
                process = subprocess.Popen(
                    aider_cmd,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT, # Redirect stderr to stdout
                    stdin=subprocess.PIPE, # Allow sending input later if needed
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
            # No queue needed if we just append to buffer directly from the thread
            output_thread = threading.Thread(target=self._read_agent_output, args=(process.stdout, output_buffer, role, agent_id), daemon=True)
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

    def _read_agent_output(self, stream, buffer, role, agent_id):
        """Reads output from a stream (Aider stdout) and appends to the buffer."""
        prefix = f"[Aider-{role}-{agent_id}]"
        try:
            for line in iter(stream.readline, ''):
                line = line.strip()
                if line:
                    buffer.append(line)
                    print(f"{prefix} {line}") # Print Aider output to console
                    # TODO: Add logic here to detect specific prompts (e.g., "Apply changes?")
                    # and potentially send input via process.stdin.write() / process.stdin.flush()
                    # This requires careful handling of blocking and potential deadlocks.
        except ValueError:
            # Handle "ValueError: readline of closed file" which can happen during shutdown
            logging.debug(f"Readline error on closed stream for agent {role}-{agent_id}. Likely shutdown.")
        except Exception as e:
            logging.error(f"Error reading agent output for {role}-{agent_id}: {e}")
        finally:
            if stream and not stream.closed:
                stream.close()
            logging.debug(f"Output reading thread finished for agent {role}-{agent_id}.")


    def _create_handoff(self, next_role, message):
        """Creates a handoff file for the next agent."""
        # Ensure handoff dir exists (might be called before start() in some scenarios)
        os.makedirs(self.handoff_dir, exist_ok=True)
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
        try:
            handoff_files = os.listdir(self.handoff_dir)
        except FileNotFoundError:
            logging.warning(f"Handoff directory '{self.handoff_dir}' not found. Skipping handoff processing.")
            return # No directory, nothing to process

        for fname in handoff_files:
            # Define roles needing handoffs based on RULES.md and potential future roles
            known_roles = ["coordinator", "architect", "developer", "tester", "refactorer", "planner", "engineer", "infra engineer"]
            if fname.endswith(".json") and any(fname.startswith(f"{role}_") for role in known_roles):
                path = os.path.join(self.handoff_dir, fname)
                try:
                    with open(path, "r") as f:
                        data = json.load(f)
                    role = data.get("role")
                    message = data.get("message")
                    if role and message:
                        logging.info(f"Processing handoff for role: {role}")
                        # Decide which type of agent to start based on role
                        if role in ["coordinator", "architect", "theorist", "skeptic", "historian"]: # Non-Aider roles
                             self._start_coordinator_agent(role, message) # Uses specific thread logic
                        elif role in ["developer", "tester", "refactorer", "planner", "engineer", "infra engineer"]: # Roles handled by Aider
                             # Potentially choose model based on task complexity or history
                             self._start_aider_agent(role, message) # Model chosen inside if not specified
                        else:
                             # This case should theoretically not be reached due to the startswith check
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
                    # Decide whether to retry or delete - delete for now
                    processed_files.append(path)

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
            self.running = False # Signal monitor loop to stop
            logging.info("Stopping AgentManager and all agents...")
            # Create a copy of items to avoid modification during iteration
            active_agents_copy = list(self.active_agents.items())
            self.active_agents.clear() # Clear original dict

            for role, agent_info in active_agents_copy:
                agent_id = agent_info.get('id', 'N/A')
                logging.info(f"Stopping agent '{role}' (ID: {agent_id})...")
                if agent_info['process']:
                    logging.debug(f"Terminating Aider process for '{role}' (ID: {agent_id})...")
                    try:
                        # Close stdin first to prevent blocking on input prompts
                        if agent_info['process'].stdin and not agent_info['process'].stdin.closed:
                            agent_info['process'].stdin.close()
                        agent_info['process'].terminate() # Send SIGTERM
                        agent_info['process'].wait(timeout=5) # Wait for termination
                        logging.info(f"Aider agent '{role}' (ID: {agent_id}) terminated.")
                    except subprocess.TimeoutExpired:
                        logging.warning(f"Aider agent '{role}' (ID: {agent_id}) did not terminate gracefully, killing.")
                        agent_info['process'].kill() # Force kill
                        # Wait briefly after kill
                        try:
                            agent_info['process'].wait(timeout=1)
                        except subprocess.TimeoutExpired:
                            logging.error(f"Failed to confirm kill for agent '{role}' (ID: {agent_id})")
                    except Exception as e:
                        logging.error(f"Error stopping process for agent '{role}' (ID: {agent_id}): {e}")
                if agent_info['thread'] and agent_info['thread'].is_alive():
                     # Output reading threads are daemons, they will exit.
                     # Coordinator/Architect threads are also daemons.
                     logging.debug(f"Thread for '{role}' (ID: {agent_id}) is a daemon, will exit.")

            logging.info("AgentManager stopped.")

    def get_active_agents_status(self) -> list:
        """Returns a list of dictionaries describing the active agents."""
        with self.lock:
            agents_data = []
            for role, agent_info in self.active_agents.items():
                # Truncate output preview for brevity
                output_preview = list(agent_info["output"])[-5:]
                agents_data.append({
                    "id": agent_info["id"],
                    "role": agent_info["role"],
                    "status": agent_info["status"],
                    "model": agent_info["model"],
                    "output_preview": output_preview,
                })
        return agents_data
