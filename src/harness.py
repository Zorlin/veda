import logging
import os
import subprocess
import time
from pathlib import Path
import json
from typing import Dict, Any, Optional, List, Tuple

import yaml

from .aider_interaction import run_aider
from .llm_interaction import get_llm_response
from .pytest_interaction import run_pytest
import anyio # Import anyio for streams
from anyio.streams.memory import MemoryObjectSendStream # Specific type hint
from .ledger import Ledger
from .vesper_mind import VesperMind
# No longer need direct UIServer import here for updates
import re # Import regex for ANSI stripping
import threading
import hashlib # Import hashlib for file hashing


class Harness:
    """
    Orchestrates the Aider-Ollama-Pytest loop with enhanced features:
    - SQLite/JSON ledger for persistent state
    - VESPER.MIND council for evaluation
    - Code review capabilities
    """

    def __init__(
        self,
        config_file: str = "config.yaml",
        max_retries: int = 5,
        work_dir: Path = Path("harness_work_dir"),
        reset_state: bool = False,
        ollama_model: Optional[str] = None,
        storage_type: str = "sqlite",  # "sqlite" or "json"
        enable_council: bool = True,
        enable_code_review: bool = False,
        # Allow overriding UI settings via init
        enable_ui: Optional[bool] = None, # Add enable_ui argument
        websocket_host: Optional[str] = None,
        websocket_port: Optional[int] = None,
        # Add stream for UI updates
        ui_send_stream: Optional[MemoryObjectSendStream] = None
    ):
        self.config_file = config_file
        self.max_retries = max_retries
        self.work_dir = work_dir
        self.config: Dict[str, Any] = self._load_config()
        
        # Override config model if CLI argument is provided
        if ollama_model:
            logging.info(f"Overriding configured Ollama model with __init__ argument: {ollama_model}")
            self.config["ollama_model"] = ollama_model

        # Override UI settings if provided in __init__ arguments
        if enable_ui is not None:
            self.config["enable_ui"] = enable_ui
            logging.info(f"UI enabled status set by __init__ argument: {enable_ui}")
        if websocket_host is not None:
            self.config["websocket_host"] = websocket_host
            logging.info(f"WebSocket host set by __init__ argument: {websocket_host}")
        if websocket_port is not None:
            self.config["websocket_port"] = websocket_port
            logging.info(f"WebSocket port set by __init__ argument: {websocket_port}")
            
        # Initialize ledger for persistent state
        self.ledger = Ledger(
            work_dir=self.work_dir,
            storage_type=storage_type
        )
        
        # Initialize VESPER.MIND council if enabled
        self.enable_council = enable_council
        if enable_council:
            try:
                self.council = VesperMind(
                    config=self.config,
                    ledger=self.ledger,
                    work_dir=self.work_dir
                )
                logging.info("VESPER.MIND council initialized successfully")
            except Exception as e:
                logging.error(f"Failed to initialize VESPER.MIND council: {e}")
                logging.info("Falling back to standard LLM evaluation")
                self.enable_council = False
                self.council = None
        else:
            self.council = None
        
        # Initialize state from ledger or create new state
        self.state = self._initialize_state(reset_state)
        
        # Code review settings
        self.enable_code_review = enable_code_review
        self.current_run_id = None

        # Store the UI send stream
        self.ui_send_stream = ui_send_stream

        # Interrupt handling state
        self._interrupt_requested: bool = False # Flag indicating a user message is pending
        self._force_interrupt: bool = False # Flag indicating the *current* Aider run should be stopped
        self._interrupt_message: Optional[str] = None # The pending user message
        self._aider_thread: Optional[threading.Thread] = None
        self._aider_interrupt_event: Optional[threading.Event] = None
        self._last_aider_output_chunk: Optional[str] = None

        # Goal prompt tracking
        self._goal_prompt_file: Optional[Path] = None # Store path to goal file if applicable
        self._last_goal_prompt_hash: Optional[str] = None # Store hash of the goal prompt content

        logging.info(f"Harness initialized. Max retries: {self.max_retries}")
        logging.info(f"Working directory: {self.work_dir.resolve()}")
        logging.info(f"Storage type: {storage_type}")
        logging.info(f"VESPER.MIND council enabled: {self.enable_council}") # Use self.enable_council
        logging.info(f"Code review enabled: {self.enable_code_review}") # Use self.enable_code_review
        # Log the final UI enabled status after config loading and potential overrides
        logging.info(f"UI enabled: {self.config.get('enable_ui', False)}")
        logging.info(f"WebSocket Host: {self.config.get('websocket_host', 'N/A')}")
        logging.info(f"WebSocket Port: {self.config.get('websocket_port', 'N/A')}")

    # Removed set_ui_server method

    def _send_ui_update(self, update: Dict[str, Any]):
        """Sends an update to the UI server via the memory stream if enabled."""
        if self.config.get("enable_ui") and self.ui_send_stream:
            # Add common context if not present
            update.setdefault("run_id", self.current_run_id)
            update.setdefault("iteration", self.state.get("current_iteration", 0) + 1) # UI shows 1-based
            # --- Added Logging ---
            log_update_preview = {k: (v[:50] + '...' if isinstance(v, str) and len(v) > 50 else v) for k, v in update.items()}
            logging.debug(f"[_send_ui_update] Attempting to send update via stream: {log_update_preview}")
            # --- End Added Logging ---
            try:
                # Send the update dictionary through the stream (non-blocking)
                self.ui_send_stream.send_nowait(update)
                # logging.debug(f"[_send_ui_update] Successfully sent update.") # Optional: log success
            except anyio.WouldBlock:
                # Should not happen with infinite buffer, but good practice
                logging.warning("UI update stream is unexpectedly blocked.")
           except anyio.BrokenResourceError:
                # This happens if the receiver (UI server listener) has closed the stream.
                # Log it but don't crash the harness.
                logging.warning("UI update stream receiver closed. Cannot send update.")
                # Optionally disable further UI updates?
                # self.ui_send_stream = None # Or set a flag
           except Exception as e:
                # Log other errors during UI update without crashing the harness
                logging.error(f"Error sending UI update via stream: {e}", exc_info=True)
       # else:
             # logging.debug("UI update skipped (UI disabled or stream not available).")

    def request_interrupt(self, message: str, interrupt_now: bool = False):
        """
        Called by the UI Server to queue user guidance or signal an immediate interrupt.

        Args:
            message: The guidance message from the user.
            interrupt_now: If True, signal the current Aider process to stop.
                           If False, queue the message for the next iteration.
        """
        log_level = logging.WARNING if interrupt_now else logging.INFO
        log_prefix = "Interrupt & Stop Aider" if interrupt_now else "Queue Guidance"

        # Log the request type and message
        logging.log(log_level, f"{log_prefix} requested by user. Message: '{message[:100]}...'")

        # Always store the message and set the requested flag
        self._interrupt_message = message
        self._interrupt_requested = True # Indicates a message is pending injection

        # Only set force_interrupt and signal the thread if interrupt_now is True
        if interrupt_now:
            self._force_interrupt = True # Indicates the *current* Aider run should stop
            if self._aider_thread and self._aider_thread.is_alive() and self._aider_interrupt_event:
                logging.warning("Signaling running Aider thread to terminate due to user request.")
                self._aider_interrupt_event.set() # Signal the event
            else:
                logging.info("Aider thread not running or already signaled.")
        else:
             # If just queuing, ensure force_interrupt is False for this specific request
             # (though it might already be True from a previous request)
             # We don't reset self._force_interrupt here, only set it if interrupt_now is true.
             pass

        # Send UI update acknowledging the request
        status_msg = "Interrupting Aider & Queuing Guidance" if interrupt_now else "Guidance Queued for Next Iteration"
        self._send_ui_update({"status": status_msg, "log_entry": f"{status_msg}: '{message[:50]}...'"})


    def _load_config(self) -> Dict[str, Any]:
        """Loads configuration from the YAML file."""
        default_config = {
            "ollama_model": "gemma3:12b", # Set default to gemma3:12b
            "ollama_api_url": "http://localhost:11434/api/generate", # TODO: Use this
            "aider_command": "aider", # Adjust if aider is not in PATH
            "aider_test_command": "pytest -v", # Default test command for Aider
            "project_dir": ".", # Directory Aider should operate on
            "ollama_request_timeout": 300, # Default timeout for Ollama requests (seconds)
            # UI Config Defaults
            "enable_ui": False,
            "websocket_host": "localhost",
            "websocket_port": 8765,
            # TODO: Add other necessary config like pytest command, etc.
        }
        config = default_config.copy()
        config_path = None # Initialize config_path

        # Handle config_file=None case explicitly
        if self.config_file is None:
            logging.info("No config file specified. Using default configuration.")
            # project_dir defaults to "." from default_config
            project_dir_path = Path(config.get("project_dir", "."))
            if not project_dir_path.is_absolute():
                project_dir_path = Path.cwd() / project_dir_path
            config["project_dir"] = str(project_dir_path.resolve())
            # IMPORTANT: When config_file is None, use work_dir directly as passed in __init__
            # Do not resolve it relative to project_dir here. Ensure it's absolute.
            self.work_dir = self.work_dir.resolve()
            logging.info(f"Using provided working directory directly: {self.work_dir}")
            # State initialization happens after this method returns in __init__
            return config
        else:
             # Proceed with loading from the specified config file
             logging.info(f"Loading configuration from {self.config_file}...")
             config_path = Path(self.config_file)
             try:
                 if config_path.is_file():
                     with open(config_path, 'r') as f:
                         user_config = yaml.safe_load(f)
                     # Check user_config *after* the 'with open' block closes the file
                     if user_config: # Ensure file is not empty and is a dict
                         if isinstance(user_config, dict):
                             config.update(user_config)
                             logging.info(f"Loaded and merged configuration from {self.config_file}")
                         else:
                             logging.warning(f"Config file {self.config_file} does not contain a valid dictionary. Using defaults.")
                     else:
                         logging.info(f"Config file {self.config_file} is empty. Using defaults.")
                 else: # This corresponds to 'if config_path.is_file():'
                     logging.warning(f"Config file {self.config_file} not found. Using default configuration.")
                     # Optionally create a default config file here
                 # try:
                 #     with open(config_path, 'w') as f:
                 #         yaml.dump(default_config, f, default_flow_style=False)
                 #     logging.info(f"Created default config file at {self.config_file}")
                 # except IOError as e_write:
                     #     logging.error(f"Could not write default config file {config_path}: {e_write}")

             except yaml.YAMLError as e:
                 logging.error(f"Error parsing config file {config_path}: {e}. Using default configuration.")
             except IOError as e:
                 logging.error(f"Error reading config file {config_path}: {e}. Using default configuration.")
             except Exception as e:
                 logging.error(f"Unexpected error loading config file {config_path}: {e}. Using default configuration.")

        # This part runs only if config_file was not None
        # Resolve project_dir (relative to CWD if needed)
        project_dir_path = Path(config.get("project_dir", "."))
        if not project_dir_path.is_absolute():
             # Assuming the script runs from the project root
             project_dir_path = Path.cwd() / project_dir_path
        config["project_dir"] = str(project_dir_path.resolve()) # Store absolute path

        # Resolve work_dir passed from __init__ relative to CWD and ensure it's absolute
        # This should happen *independently* of the project_dir
        self.work_dir = self.work_dir.resolve()
        self.work_dir.mkdir(parents=True, exist_ok=True)
        logging.info(f"Using working directory (resolved relative to CWD): {self.work_dir}")

        # State initialization happens after this method returns in __init__
        return config

    def _initialize_state(self, reset_state: bool) -> Dict[str, Any]:
        """
        Initializes the harness state.
        If reset_state is False, tries to load the latest run from the ledger.
        Otherwise, returns a fresh state.
        """
        if reset_state:
            logging.info("Resetting state as requested.")
            return {
                "current_iteration": 0,
                "prompt_history": [],
                "converged": False,
                "last_error": None,
                "run_id": None
            }
        
        # Try to get the latest run ID from the ledger
        latest_run_id = self.ledger.get_latest_run_id()
        
        if latest_run_id is not None:
            # Get run summary
            run_summary = self.ledger.get_run_summary(latest_run_id)
            
            # Check if the run is still in progress (no end_time)
            if run_summary and not run_summary.get("end_time"):
                logging.info(f"Resuming run {latest_run_id}")
                
                # Get conversation history
                history = self.ledger.get_conversation_history(latest_run_id)
                
                # Determine current iteration
                current_iteration = run_summary.get("iteration_count", 0)
                
                return {
                    "current_iteration": current_iteration,
                    "prompt_history": history,
                    "converged": run_summary.get("converged", False),
                    "last_error": run_summary.get("final_status"),
                    "run_id": latest_run_id
                }
        
        # No valid run to resume or reset requested
        logging.info("Initializing fresh state.")
        return {
            "current_iteration": 0,
            "prompt_history": [],
            "converged": False,
            "last_error": None,
            "run_id": None
        }

    def _get_file_hash(self, file_path: Path) -> Optional[str]:
        """Calculates the SHA256 hash of a file's content."""
        try:
            hasher = hashlib.sha256()
            with open(file_path, 'rb') as f:
                while chunk := f.read(4096):
                    hasher.update(chunk)
            return hasher.hexdigest()
        except FileNotFoundError:
            logging.error(f"Goal prompt file not found at {file_path} during hash calculation.")
            return None
        except IOError as e:
            logging.error(f"Error reading goal prompt file {file_path} for hashing: {e}")
            return None

    def run(self, initial_goal_prompt_or_file: str):
        """
        Runs the main Aider-Pytest-Ollama loop with enhanced features.

        Args:
            initial_goal_prompt_or_file: The initial goal prompt string OR path to a file containing the goal.
        """
        logging.info("Starting harness run...")
        self._send_ui_update({"status": "Starting Run", "log_entry": "Harness run initiated."})

        # Determine if input is a file path or a string
        goal_prompt_path = Path(initial_goal_prompt_or_file)
        current_goal_prompt = ""
        if goal_prompt_path.is_file():
            logging.info(f"Loading initial goal from file: {goal_prompt_path}")
            self._goal_prompt_file = goal_prompt_path.resolve() # Store absolute path
            try:
                current_goal_prompt = self._goal_prompt_file.read_text()
                self._last_goal_prompt_hash = self._get_file_hash(self._goal_prompt_file)
                logging.info(f"Initial goal prompt hash: {self._last_goal_prompt_hash}")
            except Exception as e:
                logging.error(f"Failed to read initial goal prompt file {self._goal_prompt_file}: {e}. Aborting.")
                self._send_ui_update({"status": "Error", "log_entry": f"Failed to read goal file: {e}"})
                return {"run_id": None, "iterations": 0, "converged": False, "final_status": f"ERROR: Failed to read goal file {self._goal_prompt_file}"}
        else:
            logging.info("Using provided string as initial goal prompt.")
            current_goal_prompt = initial_goal_prompt_or_file
            self._goal_prompt_file = None # Not using a file
            self._last_goal_prompt_hash = None
        
        # Initialize current_prompt before history check
        current_prompt = current_goal_prompt

        # Start a new run in the ledger if we don't have an active one
        if self.state["run_id"] is None:
            self.current_run_id = self.ledger.start_run(
                current_goal_prompt, # Use the loaded/provided goal
                self.max_retries,
                self.config
            )
            self.state["run_id"] = self.current_run_id
            logging.info(f"Started new run with ID {self.current_run_id}")
        else:
            self.current_run_id = self.state["run_id"]
            logging.info(f"Continuing run with ID {self.current_run_id}")
        
        # Initialize prompt history only if starting fresh (using current_goal_prompt)
        if self.state["current_iteration"] == 0 and not self.state["prompt_history"]:
            logging.info("Initializing prompt history with the initial goal.")
            # current_prompt is already set from file/string loading above
            # Ensure history is clean before adding the first prompt
            self.state["prompt_history"] = [{"role": "user", "content": current_goal_prompt}]
            # Add to ledger (using current_goal_prompt)
            self.ledger.add_message(self.current_run_id, None, "user", current_goal_prompt)
        elif self.state["prompt_history"]:
            # If resuming, check if the last message was a user prompt and update current_prompt
            last_message = self.state["prompt_history"][-1]
            if last_message.get("role") == "user":
                current_prompt = last_message["content"] # Update current_prompt for this iteration
                logging.info(f"Resuming run from iteration {self.state['current_iteration'] + 1}. Last user prompt retrieved from history.")
            else: # Last message is from assistant or system
                # This means the previous iteration's Aider run completed, but didn't generate a new user prompt.
                # Or a system message was the last one.
                # Start a fresh run with the potentially updated goal.
                logging.info("Previous run concluded (last message was from assistant). Starting a fresh run with the current goal.")
                # current_goal_prompt is already set from file/string loading above
                # Reset state for a fresh run
                self.state["current_iteration"] = 0
                self.state["prompt_history"] = [{"role": "user", "content": current_goal_prompt}]
                self.state["converged"] = False
                self.state["last_error"] = None
                # Start a new run in the ledger (using current_goal_prompt)
                self.current_run_id = self.ledger.start_run(
                    current_goal_prompt,
                    self.max_retries,
                    self.config
                )
                self.state["run_id"] = self.current_run_id
                # Add initial goal message to ledger for the new run
                self.ledger.add_message(self.current_run_id, None, "user", current_goal_prompt)
                # current_prompt remains the initial current_goal_prompt set earlier
        else:
            # Should not happen if initialization is correct, but handle defensively
            logging.warning("State indicates resumption but history is empty. Starting with current goal.")
            # current_prompt remains the initial current_goal_prompt set earlier
            # current_goal_prompt is already set from file/string loading above
            self.state["prompt_history"] = [{"role": "user", "content": current_goal_prompt}]
            # Add initial goal message to ledger (using current_goal_prompt)
            self.ledger.add_message(self.current_run_id, None, "user", current_goal_prompt)
            # current_prompt remains the initial current_goal_prompt set earlier

        # Track recent diffs to detect stuck cycles
        recent_diffs = [] # Store the last few non-empty diffs
        stuck_cycle_threshold = self.config.get("stuck_cycle_threshold", 2) # Number of consecutive identical non-empty diffs to trigger abort

        while (
            self.state["current_iteration"] < self.max_retries
            and not self.state["converged"]
        ):
            iteration = self.state["current_iteration"]
            iteration_num_display = iteration + 1
            iteration_interrupted = False # Flag specific to this iteration

            # --- Check for Goal Prompt File Changes (if applicable) ---
            if self._goal_prompt_file:
                new_hash = self._get_file_hash(self._goal_prompt_file)
                if new_hash is not None and new_hash != self._last_goal_prompt_hash:
                    logging.warning(f"Change detected in goal prompt file: {self._goal_prompt_file}")
                    self._send_ui_update({"status": "Goal Updated", "log_entry": f"Goal prompt file '{self._goal_prompt_file.name}' changed. Reloading..."})
                    try:
                        current_goal_prompt = self._goal_prompt_file.read_text()
                        self._last_goal_prompt_hash = new_hash
                        logging.info("Successfully reloaded goal prompt.")
                        # Option 1: Inject as guidance (similar to interrupt)
                        # self._interrupt_message = f"[Goal Reloaded]\n{current_goal_prompt}"
                        # self._interrupt_requested = True
                        # Option 2: Update the 'initial_goal_prompt' variable used later in evaluations/retries
                        # Let's use Option 2 for now, as it affects the core reference goal.
                        # The 'current_prompt' for the *next* Aider run will be based on this updated goal
                        # if the loop continues (e.g., after a RETRY).
                        # We also need to update the initial goal stored in the ledger run record? No, ledger is immutable history.
                        # We should add a message to the history/ledger indicating the goal changed.
                        goal_change_message = f"[System Event] Goal prompt reloaded from {self._goal_prompt_file.name} at Iteration {iteration_num_display}."
                        self.state["prompt_history"].append({"role": "system", "content": goal_change_message})
                        self.ledger.add_message(self.current_run_id, None, "system", goal_change_message) # Associate with run, not specific iteration
                        # Update the variable used in evaluation prompts etc.
                        initial_goal_prompt = current_goal_prompt
                        self._send_ui_update({"status": "Goal Updated", "log_entry": "Goal prompt reloaded successfully."})

                    except Exception as e:
                        logging.error(f"Failed to reload goal prompt file {self._goal_prompt_file}: {e}")
                        self._send_ui_update({"status": "Error", "log_entry": f"Failed to reload goal file: {e}. Continuing with previous goal."})
                        # Continue with the old goal prompt in memory

            # --- Check for Pending User Guidance (Inject before starting Aider) ---
            # Use 'current_prompt' which holds the prompt intended for the *next* Aider run
            if self._interrupt_requested and self._interrupt_message is not None:
                logging.warning(f"--- Injecting User Guidance before Iteration {iteration_num_display} ---")
                self._send_ui_update({"status": "Injecting Guidance", "log_entry": f"Injecting user guidance into prompt for Iteration {iteration_num_display}."})

                interrupt_msg = self._interrupt_message
                guidance_prefix = "[User Guidance]" # Prefix to clearly mark user input in history

                # Modify the 'current_prompt' variable which holds the prompt for the upcoming Aider run
                # Place guidance *before* the previous prompt content for priority
                # Ensure 'current_prompt' reflects the latest state before modification
                if not self.state["prompt_history"] or self.state["prompt_history"][-1]["role"] != "user":
                     # If history is empty or last message wasn't user, base next prompt on current_goal_prompt
                     base_prompt_for_guidance = current_goal_prompt
                else:
                     # Otherwise, base it on the last user message in history
                     base_prompt_for_guidance = self.state["prompt_history"][-1]["content"]

                current_prompt = f"{guidance_prefix}\n{interrupt_msg}\n\n---\n(Continuing task with this guidance)\n---\n\n{base_prompt_for_guidance}"

                logging.info(f"Updated prompt after injecting guidance:\n{current_prompt}")
                self._send_ui_update({"status": "Prompt Updated", "log_entry": "Prompt updated with user guidance."})

                # Add guidance message to history and ledger (associated with the *upcoming* iteration)
                # Use a distinct role or prefix for clarity
                guidance_history_entry = {"role": "user", "content": f"{guidance_prefix} {interrupt_msg}"}
                self.state["prompt_history"].append(guidance_history_entry)
                # Associate with the run, but not a specific completed iteration yet
                self.ledger.add_message(self.current_run_id, None, "user", f"{guidance_prefix} {interrupt_msg}")

                # Reset flags now that the message has been incorporated
                self._interrupt_requested = False
                self._interrupt_message = None
                # Also reset force_interrupt here. The *reason* for the force (the user message)
                # has been handled by injecting it. The Aider thread might still be stopping
                # from a signal sent earlier, but we don't want the harness loop logic
                # to think it's *still* under a forced condition unless another interrupt(force=True) comes in.
                self._force_interrupt = False
                logging.info("User guidance injected and flags reset.")


            # --- Start Iteration ---
            logging.info(f"--- Starting Iteration {iteration_num_display} ---")
            self._send_ui_update({"status": f"Starting Iteration {iteration_num_display}", "iteration": iteration_num_display, "log_entry": f"Starting Iteration {iteration_num_display}"})

            # Start iteration in ledger
            iteration_id = self.ledger.start_iteration(
                self.current_run_id,
                iteration + 1,
                current_prompt
            )

            try:
                # --- 1. Run Aider (in a separate thread) ---
                logging.info("Starting Aider thread...")
                # Clear the Aider output in the UI at the start of a new iteration
                self._send_ui_update({"type": "aider_output_clear"})
                # Update status
                self._send_ui_update({"status": "Running Aider", "log_entry": "Invoking Aider..."})

                self._aider_interrupt_event = threading.Event() # Create event for this run
                aider_result = {"diff": None, "error": None} # Dictionary to store result from thread

                # Define the callback for streaming Aider output to the UI
                ansi_escape_pattern = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
                def ui_output_callback(chunk: str):
                    """
                    Callback function to process and send Aider output chunks to the UI.
                    Strips ANSI codes and prevents sending duplicate consecutive chunks.
                    """
                    # Strip ANSI escape codes
                    stripped_chunk = ansi_escape_pattern.sub('', chunk)

                    # Basic handling of backspaces and carriage returns for cleaner output
                    # This is a simple approach; full terminal emulation is complex.
                    # Replace backspace + char with nothing, handle standalone backspace/CR
                    processed_chunk = re.sub(r'.\b', '', stripped_chunk) # Remove char before backspace
                    processed_chunk = processed_chunk.replace('\b', '') # Remove remaining backspaces
                    # Consider replacing \r with \n or removing based on UI needs
                    # For now, let's keep \r if it might be used for progress bars,
                    # but be aware it can cause overwriting issues in simple text logs.
                    # processed_chunk = processed_chunk.replace('\r', '\n')

                    # Only send if the processed chunk is non-empty and different from the last one
                    if processed_chunk and processed_chunk != self._last_aider_output_chunk:
                        # Send output chunk with a specific type identifier
                        self._send_ui_update({"type": "aider_output", "chunk": processed_chunk})
                        self._last_aider_output_chunk = processed_chunk # Update last sent chunk tracker
                    elif not processed_chunk:
                        # Log if the chunk becomes empty after processing, but don't send
                        logging.debug("Skipping empty chunk after ANSI/control code processing.")
                    else:
                        # Log skipped duplicate chunk for debugging
                        logging.debug(f"Skipping duplicate Aider output chunk: {processed_chunk[:50]}...")


                def aider_thread_target():
                    """Target function for the Aider thread."""
                    try:
                        diff, error = run_aider(
                            prompt=current_prompt,
                            config=self.config,
                            history=self.state["prompt_history"][:-1], # History up to the current prompt
                            work_dir=self.config["project_dir"],
                            interrupt_event=self._aider_interrupt_event, # Pass the event
                            output_callback=ui_output_callback # Pass the callback
                        )
                        aider_result["diff"] = diff
                        aider_result["error"] = error
                    except Exception as e:
                        logging.exception("Exception in Aider thread")
                        aider_result["error"] = f"Aider thread exception: {e}"

                self._aider_thread = threading.Thread(target=aider_thread_target)
                self._aider_thread.start()

                # Monitor the thread and check for forced interrupts
                while self._aider_thread.is_alive():
                    # Check frequently for forced interrupt signal
                    if self._force_interrupt and self._aider_interrupt_event and not self._aider_interrupt_event.is_set():
                         logging.warning("Forced interrupt detected while Aider running. Signaling thread.")
                         self._aider_interrupt_event.set() # Signal the thread to stop

                    # Wait for a short period before checking again
                    self._aider_thread.join(timeout=0.2) # Check every 200ms

                # Aider thread finished or was interrupted
                self._aider_thread = None # Clear thread reference
                self._aider_interrupt_event = None # Clear event reference
                self._last_aider_output_chunk = None # Reset last chunk tracker after Aider finishes

                aider_diff = aider_result["diff"]
                aider_error = aider_result["error"]

                # Check if Aider was forcefully interrupted (error is "INTERRUPTED")
                if aider_error == "INTERRUPTED":
                    logging.warning(f"Aider run for Iteration {iteration_num_display} was stopped by user interrupt signal.")
                    self._send_ui_update({"status": "Aider Interrupted", "log_entry": "Aider process stopped by user interrupt signal."})
                    iteration_interrupted = True # Mark iteration as interrupted

                    # The user's guidance message (if any) was already stored in self._interrupt_message
                    # and will be injected at the *start* of the next loop cycle by the logic above.
                    # No need to modify the prompt or history here.

                    # Complete the iteration record in the ledger, noting the interruption
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        aider_diff, # Diff might be partial or None
                        "[No tests run due to interrupt]",
                        False, # Assume tests didn't pass
                        "INTERRUPTED", # Special verdict
                        "Aider process stopped by user signal."
                    )

                    # Skip the rest of the loop (pytest, eval) for this iteration
                    continue # Go to the next iteration immediately

                # Handle other Aider errors
                elif aider_error:
                    logging.error(f"Aider failed: {aider_error}")
                    self.state["last_error"] = f"Aider failed: {aider_error}"
                    self._send_ui_update({"status": "Error", "log_entry": f"Aider failed: {aider_error}"})
                    # Update ledger with error
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        None,
                        f"Error: {aider_error}",
                        False,
                        "FAILURE",
                        f"Aider failed: {aider_error}"
                    )
                    break

                if aider_diff is None:
                    logging.error("Aider returned None for diff without error. Stopping.")
                    self.state["last_error"] = "Aider returned None diff unexpectedly."
                    self._send_ui_update({"status": "Error", "log_entry": "Aider returned None diff unexpectedly."})
                    # Update ledger with error
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        None,
                        "Aider returned None diff unexpectedly.",
                        False,
                        "FAILURE",
                        "Aider returned None diff unexpectedly."
                    )
                    break

                # --- Aider finished normally (not interrupted forcefully) ---
                log_diff_summary = (aider_diff[:200] + '...' if len(aider_diff) > 200 else aider_diff) if aider_diff else '[No changes detected]'
                logging.info(f"Aider finished. Diff summary:\n{log_diff_summary}")
                self._send_ui_update({"status": "Aider Finished", "aider_diff": aider_diff, "log_entry": f"Aider finished. Diff:\n{log_diff_summary}"})

                # Add Aider's response to history and ledger
                assistant_message = aider_diff if aider_diff is not None else "[Aider encountered an error or produced no output]"
                if aider_diff == "":
                    assistant_message = "[Aider made no changes]"
                self.state["prompt_history"].append({"role": "assistant", "content": assistant_message})
                self.ledger.add_message(self.current_run_id, iteration_id, "assistant", assistant_message)

                # Check for stuck cycle (consecutive identical non-empty diffs)
                if aider_diff and aider_diff.strip(): # Only check non-empty diffs
                    recent_diffs.append(aider_diff)
                    if len(recent_diffs) > stuck_cycle_threshold:
                        recent_diffs.pop(0) # Keep only the last few
                    
                    if len(recent_diffs) == stuck_cycle_threshold and len(set(recent_diffs)) == 1:
                        logging.error(f"Stuck cycle detected: Aider produced the same diff {stuck_cycle_threshold} times consecutively. Aborting.")
                        self.state["last_error"] = "Stuck cycle detected (repeated diff)"
                        self._send_ui_update({"status": "Error", "log_entry": "Stuck cycle detected. Aborting."})
                        self.ledger.complete_iteration(
                            self.current_run_id, iteration_id, aider_diff,
                            "[No tests run due to stuck cycle]", False, "FAILURE", 
                            "Stuck cycle detected (repeated diff)"
                        )
                        break # Exit the main loop

                # --- 2. Run Pytest ---
                logging.info("Running pytest...")
                self._send_ui_update({"status": "Running Pytest", "log_entry": "Running pytest..."})
                pytest_passed, pytest_output = run_pytest(self.config["project_dir"])
                summary_output = (pytest_output[:500] + '...' if len(pytest_output) > 500 else pytest_output)
                logging.info(f"Pytest finished. Passed: {pytest_passed}\nOutput (truncated):\n{summary_output}")
                self._send_ui_update({
                    "status": "Pytest Finished",
                    "pytest_passed": pytest_passed,
                    "pytest_output": pytest_output,
                    "log_entry": f"Pytest finished. Passed: {pytest_passed}. Output:\n{summary_output}"
                })

                # 3. Evaluate with VESPER.MIND council or standard LLM
                evaluation_status = "Evaluating (Council)" if self.enable_council and self.council else "Evaluating (LLM)"
                self._send_ui_update({"status": evaluation_status, "log_entry": evaluation_status + "..."})
                try:
                    if self.enable_council and self.council:
                        logging.info("Evaluating with VESPER.MIND council...")
                        verdict, suggestions, council_results = self.council.evaluate_iteration(
                            self.current_run_id,
                            iteration_id,
                            current_goal_prompt, # Use the potentially updated goal
                            aider_diff if aider_diff is not None else "",
                            pytest_output,
                            pytest_passed,
                            self.state["prompt_history"]
                        )
                        logging.info(f"VESPER.MIND council verdict: {verdict}")
                        self._send_ui_update({"status": "Council Evaluated", "verdict": verdict, "suggestions": suggestions, "log_entry": f"Council verdict: {verdict}"})
                        
                        # Generate changelog if successful
                        if verdict == "SUCCESS":
                            try:
                                changelog = self.council.generate_changelog(
                                    self.current_run_id,
                                    iteration_id,
                                    verdict
                                )
                                logging.info(f"Generated changelog:\n{changelog}")
                                
                                # Save changelog to file
                                changelog_dir = self.work_dir / "changelogs"
                                changelog_dir.mkdir(exist_ok=True)
                                changelog_file = changelog_dir / f"changelog_run{self.current_run_id}_iter{iteration_id}.md"
                                with open(changelog_file, 'w') as f:
                                    f.write(changelog)
                            except Exception as e:
                                logging.error(f"Error generating changelog: {e}")
                    else:
                        # Standard LLM evaluation
                        logging.info("Evaluating outcome with standard LLM...")
                        verdict, suggestions = self._evaluate_outcome(
                            current_goal_prompt, # Use the potentially updated goal
                            aider_diff if aider_diff is not None else "",
                            pytest_output,
                            pytest_passed
                        )
                        logging.info(f"LLM evaluation result: Verdict={verdict}, Suggestions='{suggestions}'")
                        self._send_ui_update({"status": "LLM Evaluated", "verdict": verdict, "suggestions": suggestions, "log_entry": f"LLM verdict: {verdict}"})
                except Exception as e:
                    logging.error(f"Error during evaluation: {e}")
                    self._send_ui_update({"status": "Error", "log_entry": f"Error during evaluation: {e}"})
                    logging.info("Falling back to standard LLM evaluation")
                    verdict, suggestions = self._evaluate_outcome(
                        current_goal_prompt, # Use the potentially updated goal
                        aider_diff if aider_diff is not None else "",
                        pytest_output,
                        pytest_passed
                    )
                    logging.info(f"Fallback LLM evaluation result: Verdict={verdict}, Suggestions='{suggestions}'")

                # Update ledger with iteration results
                self.ledger.complete_iteration(
                    self.current_run_id,
                    iteration_id,
                    aider_diff,
                    pytest_output,
                    pytest_passed,
                    verdict,
                    suggestions
                )

                # 4. Decide next step based on verdict
                if verdict == "SUCCESS":
                    logging.info("Evaluation confirms SUCCESS.")
                    self.state["converged"] = True # Mark as converged first

                    # Run code review if enabled *after* confirming success
                    if self.enable_code_review:
                        logging.info("Running code review...")
                        self._send_ui_update({"status": "Running Code Review", "log_entry": "Running code review..."})
                        try:
                            # run_code_review now saves the file itself
                            self.run_code_review(
                                current_goal_prompt, # Use the potentially updated goal
                                aider_diff if aider_diff is not None else "",
                                pytest_output
                            )
                            logging.info(f"Code review completed and saved.")
                            self._send_ui_update({"status": "Code Review Complete", "log_entry": "Code review completed and saved."})
                        except Exception as review_err:
                            logging.error(f"Code review failed: {review_err}", exc_info=True)
                            # Log the error, but don't stop the loop since the main goal succeeded
                            self._send_ui_update({"status": "Error", "log_entry": f"Code review failed: {review_err}"})

                    # Stop the loop after success (and optional review)
                    logging.info("Stopping loop due to SUCCESS verdict.")
                    self._send_ui_update({"status": "SUCCESS", "log_entry": "Converged: SUCCESS"})
                    break # Exit the loop

                elif verdict == "RETRY":
                    logging.info("Evaluation suggests RETRY.")
                    self._send_ui_update({"status": "RETRY Suggested", "log_entry": f"RETRY suggested. Suggestions:\n{suggestions}"})
                    if self.state["current_iteration"] + 1 >= self.max_retries:
                        logging.warning(f"Retry suggested, but max retries ({self.max_retries}) reached. Stopping.")
                        self._send_ui_update({"status": "Max Retries Reached", "log_entry": "Max retries reached after RETRY verdict."})
                        self.state["last_error"] = "Max retries reached after RETRY verdict."
                        break

                    logging.info("Creating retry prompt...")
                    current_prompt = self._create_retry_prompt(
                        current_goal_prompt, # Use the potentially updated goal
                        aider_diff if aider_diff is not None else "",
                        pytest_output,
                        suggestions
                    )
                    self.state["prompt_history"].append({"role": "user", "content": current_prompt})
                    self.ledger.add_message(self.current_run_id, None, "user", current_prompt)
                    logging.debug(f"Next prompt for Aider:\n{current_prompt}")
                else:  # verdict == "FAILURE"
                    logging.error(f"Structural failure detected. Stopping loop. Reason: {suggestions}")
                    self.state["last_error"] = f"Evaluation reported FAILURE: {suggestions}"
                    self._send_ui_update({"status": "FAILURE", "log_entry": f"FAILURE detected: {suggestions}"})
                    self.state["converged"] = False
                    break

            except Exception as e:
                # Ensure thread cleanup even if other parts of the loop fail
                if self._aider_thread and self._aider_thread.is_alive():
                    logging.error("Cleaning up Aider thread due to main loop exception.")
                    if self._aider_interrupt_event:
                        self._aider_interrupt_event.set()
                    self._aider_thread.join(timeout=1.0) # Brief wait
                self._aider_thread = None
                self._aider_interrupt_event = None

                logging.exception(f"Critical error during iteration {iteration + 1}: {e}")
                self.state["last_error"] = str(e)
                self._send_ui_update({"status": "Critical Error", "log_entry": f"Critical error: {e}"})
                
                # Update ledger with error
                try:
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        None,
                        f"Exception: {str(e)}",
                        False,
                        "FAILURE",
                        f"Internal error: {str(e)}"
                    )
                except Exception as ledger_error:
                    logging.error(f"Failed to update ledger with error: {ledger_error}")
                
                break

            finally:
                # Only increment iteration if it wasn't interrupted,
                # otherwise, the 'continue' statement handles moving to the next loop cycle
                # which effectively retries the *same* iteration number with the new prompt.
                # Let's adjust this: Always increment, but the prompt carries the context.
                self.state["current_iteration"] += 1
                # State is saved implicitly via ledger updates and end_run below
                # No need for sleep here unless debugging rate limiting issues
                # time.sleep(1)

        # End of loop
        final_log_entry = ""
        if self.state["converged"]:
            logging.info(f"Harness finished successfully after {self.state['current_iteration']} iterations.")
            final_log_entry = f"Harness finished: SUCCESS after {self.state['current_iteration']} iterations."
            final_status = "SUCCESS"
        elif self.state["current_iteration"] >= self.max_retries:
            logging.warning(f"Harness stopped after reaching max retries ({self.max_retries}).")
            final_status = f"MAX_RETRIES_REACHED: {self.state.get('last_error', 'Unknown error')}"
            final_log_entry = f"Harness finished: MAX_RETRIES_REACHED. Last error: {self.state.get('last_error', 'Unknown error')}"
        else:
            logging.error(f"Harness stopped prematurely due to error: {self.state.get('last_error', 'Unknown error')}")
            final_log_entry = f"Harness finished: ERROR. Last error: {self.state.get('last_error', 'Unknown error')}"
            final_status = f"ERROR: {self.state.get('last_error', 'Unknown error')}"

        # Update run status in ledger
        self.ledger.end_run(
            self.current_run_id,
            self.state["converged"],
            final_status
        )
        
        self._send_ui_update({"status": final_status, "log_entry": final_log_entry, "converged": self.state["converged"]})
        logging.info("Harness run complete.")
        
        # Return summary
        return {
            "run_id": self.current_run_id,
            "iterations": self.state["current_iteration"],
            "converged": self.state["converged"],
            "final_status": final_status
        }


    def _evaluate_outcome(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> Tuple[str, str]:
        """
        Evaluates the outcome of an iteration using the standard LLM.
        This is used when the VESPER.MIND council is disabled.

        Args:
            initial_goal: The original goal prompt.
            aider_diff: The diff generated by Aider in the last iteration.
            pytest_output: The output from pytest.
            pytest_passed: Boolean indicating if pytest passed.

        Returns:
            A tuple containing:
            - str: The verdict ("SUCCESS", "RETRY", "FAILURE").
            - str: Suggestions from the LLM (empty if not RETRY).
        """
        evaluation_prompt = self._create_evaluation_prompt(
            initial_goal,
            self.state["prompt_history"],
            aider_diff,
            pytest_output,
            pytest_passed
        )
        try:
            # Enhanced system prompt for better evaluation
            evaluation_system_prompt = """You are an expert software development assistant and test harness evaluator.
Analyze the provided goal, history, code changes (diff), and test results.
Determine if the changes meet the goal and tests pass.

Consider:
1. Do the changes address the specific requirements in the goal?
2. Do all tests pass? If not, are the failures related to the changes?
3. Is the code well-structured, maintainable, and following best practices?
4. Are there any potential issues or edge cases not covered?

Respond in the following format:
Verdict: [SUCCESS|RETRY|FAILURE]
Rationale: [Brief explanation of your verdict]
Suggestions: [Provide concise, actionable suggestions ONLY if verdict is RETRY, otherwise leave blank]

SUCCESS = Goal achieved and tests pass
RETRY = Changes need improvement but are on the right track
FAILURE = Fundamental issues that require a different approach
"""

            # Use a lower temperature for evaluation to get more consistent results
            ollama_options = self.config.get("ollama_options", {}).copy()
            ollama_options["temperature"] = 0.3
            
            llm_evaluation_response = get_llm_response(
                evaluation_prompt,
                {**self.config, "ollama_options": ollama_options},
                history=None,
                system_prompt=evaluation_system_prompt
            )
            logging.debug(f"LLM Evaluation Response:\n{llm_evaluation_response}")

            # Parse the LLM response
            verdict_match = re.search(r"Verdict:\s*(SUCCESS|RETRY|FAILURE)", llm_evaluation_response, re.IGNORECASE)
            rationale_match = re.search(r"Rationale:\s*(.*?)(?=\n\n|\nSuggestions:|\Z)", llm_evaluation_response, re.IGNORECASE | re.DOTALL)
            suggestions_match = re.search(r"Suggestions:\s*(.*)", llm_evaluation_response, re.IGNORECASE | re.DOTALL)

            if verdict_match:
                verdict = verdict_match.group(1).upper()
                rationale = rationale_match.group(1).strip() if rationale_match else "No rationale provided."
                suggestions = suggestions_match.group(1).strip() if suggestions_match else ""
                
                # Ensure suggestions are only returned if verdict is RETRY
                if verdict != "RETRY":
                    suggestions = ""
                # Don't include rationale in suggestions for test compatibility
                
                logging.info(f"LLM evaluation parsed: Verdict={verdict}, Rationale='{rationale[:100]}...'")
                return verdict, suggestions
            else:
                logging.warning(f"Could not parse verdict from LLM evaluation response. Defaulting to RETRY.")
                verdict = "RETRY"
                suggestions = "LLM response format was invalid. Please review the previous attempt and try again."
                return verdict, suggestions

        except Exception as e:
            logging.error(f"Error during LLM evaluation: {e}. Defaulting to RETRY.")
            verdict = "RETRY"
            suggestions = f"An error occurred during the evaluation step ({e}). Please review the previous code changes and test results, then try to improve the code to meet the original goal."
            return verdict, suggestions


    def _create_evaluation_prompt(
        self,
        initial_goal: str,
        history: List[Dict[str, str]],
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> str:
        """Creates an enhanced prompt for the LLM evaluation step."""
        # Create a concise history string for the prompt, showing last few turns
        history_limit = 3
        limited_history = history[-(history_limit * 2):] if len(history) > history_limit * 2 else history
        history_str = "\n".join([f"{msg['role'].upper()}: {msg['content'][:300]}{'...' if len(msg['content']) > 300 else ''}"
                                 for msg in limited_history])

        # Determine if this is the first iteration
        is_first_iteration = self.state["current_iteration"] == 0
        
        # Adjust evaluation criteria based on iteration number
        if is_first_iteration:
            iteration_context = "This is the first iteration. Focus on whether the implementation is on the right track, even if not perfect."
        else:
            iteration_context = f"This is iteration {self.state['current_iteration'] + 1} of maximum {self.max_retries}. Consider the progress made across iterations."

        prompt = f"""
Analyze the results of an automated code generation step in a test harness.

Initial Goal:
{initial_goal}

Iteration Context:
{iteration_context}

Conversation History (summary of last {len(limited_history)} exchanges):
{history_str}

Last Code Changes (diff):
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
Status: {'PASSED' if pytest_passed else 'FAILED'}
```
{pytest_output if pytest_output else "[No output captured]"}
```

Based on the initial goal, the conversation history, the latest code changes (diff), and the test results, evaluate the outcome.

Detailed Evaluation Criteria:
1. Goal Alignment: Do the changes directly address the requirements in the initial goal?
2. Test Results: Do all tests pass? If not, what specific issues are causing failures?
3. Code Quality: Is the code well-structured, maintainable, and following best practices?
4. Completeness: Does the implementation fully satisfy the goal, or are there missing elements?
5. Edge Cases: Are there potential issues or edge cases not addressed?

Respond using the following format:

Verdict: [SUCCESS|RETRY|FAILURE]
Rationale: [Brief explanation of your verdict, considering the evaluation criteria]
Suggestions: [Provide specific, actionable suggestions ONLY if the verdict is RETRY. Explain exactly what needs to be fixed and how. If SUCCESS or FAILURE, leave this blank.]
"""
        return prompt.strip()

    def _create_retry_prompt(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        suggestions: str
    ) -> str:
        """
        Creates an enhanced user prompt for the next Aider attempt based on evaluation suggestions.
        """
        # Determine iteration number for context
        current_iteration = self.state["current_iteration"]
        max_retries = self.max_retries
        
        retry_prompt = f"""
The previous attempt to achieve the goal needs improvement (Iteration {current_iteration + 1} of {max_retries}):

Original Goal:
"{initial_goal}"

Last Code Changes:
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
```
{pytest_output if pytest_output else "[No output captured]"}
```

Evaluation and Specific Suggestions for Improvement:
{suggestions if suggestions else "No specific suggestions were provided by the evaluation. Please analyze the previous changes and test results to identify issues and determine next steps."}

Your Task:
1. Carefully review the suggestions and test results
2. Address each specific issue mentioned in the evaluation
3. Ensure all tests pass
4. Make sure your changes fully satisfy the original goal

Focus on implementing the suggested improvements while maintaining code quality and best practices.
"""
        # Add a specific note if the evaluation itself failed
        if "An error occurred during the evaluation step" in suggestions:
            retry_prompt += "\n\nNote: The automated evaluation step encountered an error, so the suggestions are generic. Please carefully review the goal, the last code changes, and the test results yourself to decide how to proceed."
        
        # Add context about remaining iterations
        remaining = max_retries - current_iteration - 1
        if remaining <= 2:
            retry_prompt += f"\n\nIMPORTANT: You have only {remaining} {'iteration' if remaining == 1 else 'iterations'} remaining. Please focus on the most critical issues first."
        
        return retry_prompt.strip()

    # --- Code Review ---
    def run_code_review(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str
    ) -> str:
        """
        Runs a code review on successful changes using Aider.

        Args:
            initial_goal: The original goal prompt.
            aider_diff: The diff generated by Aider.
            pytest_output: The output from pytest.

        Returns:
            The code review result as a string.
        """
        logging.info("Running code review...")

        # Get the configured code review model
        model_name = self.config.get("code_review_model")
        if not model_name:
            logging.warning("No 'code_review_model' specified in config. Falling back to 'ollama_model'.")
            model_name = self.config.get("ollama_model", "gemma3:12b") # Fallback

        logging.info(f"Using model '{model_name}' for code review.")

        # Create code review prompt
        review_prompt = f"""
Act as a senior code reviewer. Review the following code changes that were made to achieve this goal:

Goal: {initial_goal}

Code Changes:
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results (showing PASSED):
```
{pytest_output}
```

Provide a thorough code review that includes:
1. Overall assessment of code quality (readability, maintainability, efficiency).
2. Specific strengths of the implementation.
3. Areas for potential improvement (e.g., alternative approaches, optimizations, style suggestions).
4. Any potential bugs, edge cases, or security concerns missed by the tests.
5. Adherence to best practices and Python conventions.
6. Suggestions for future enhancements or refactorings.

Format your review as a professional code review document using Markdown with the following sections:
- Summary
- Strengths
- Areas for Improvement
- Code Quality Assessment
- Security Considerations
- Future Recommendations

Use headings, bullet points, and code snippets where appropriate.
"""

        try:
            # Use the configured LLM directly via get_llm_response
            review_system_prompt = """You are an expert code reviewer with years of experience.
Provide thorough, constructive code reviews that highlight both strengths and areas for improvement.
Focus on code quality, maintainability, performance, and adherence to best practices.
Be specific and provide concrete examples and suggestions.
Format your review as a professional markdown document with clear sections and specific examples."""

            # Use specific options for code review, potentially different from general Ollama options
            review_ollama_options = self.config.get("ollama_options", {}).copy()
            review_ollama_options["temperature"] = 0.5 # Slightly lower temp for more focused review
            
            # Pass the specific model name to get_llm_response
            # Note: get_llm_response needs to handle model names like "ollama/gemma3:12b"
            # or potentially integrate with different APIs if non-Ollama models are used.
            # Assuming get_llm_response can handle the model_name format for now.
            review_config = self.config.copy()
            review_config["ollama_model"] = model_name # Tell get_llm_response which model to use
            review_config["ollama_options"] = review_ollama_options

            review_result = get_llm_response(
                review_prompt,
                review_config, # Pass the modified config with the review model
                history=None, # Review is based on the current state, not conversation history
                system_prompt=review_system_prompt
            )

            # Add header to the review
            timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
            header = f"# Code Review\n\n**Run ID:** {self.current_run_id}\n**Date:** {timestamp}\n\n**Reviewer:** AI Code Reviewer ({model_name})\n\n---\n\n"

            full_review = header + review_result
            
            # Save the review to a file
            review_dir = self.work_dir / "reviews"
            review_dir.mkdir(exist_ok=True)
            review_file = review_dir / f"review_run{self.current_run_id}_iter{self.state['current_iteration']}.md"
            with open(review_file, 'w') as f:
                f.write(full_review)
            
            return full_review

        except Exception as e:
            logging.error(f"Error during code review generation: {e}")
            error_review = f"# Code Review\n\n**Run ID:** {self.current_run_id}\n**Date:** {time.strftime('%Y-%m-%d %H:%M:%S')}\n\n**Reviewer:** AI Code Reviewer (Error)\n\n---\n\nError during code review generation: {e}\n\nPlease review the code manually."
            
            # Save the error review to a file
            review_dir = self.work_dir / "reviews"
            review_dir.mkdir(exist_ok=True)
            review_file = review_dir / f"review_run{self.current_run_id}_iter{self.state['current_iteration']}_error.md"
            with open(review_file, 'w') as f:
                f.write(error_review)
            
            return error_review


# Example usage (for testing purposes, normally called from main.py)
if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s")
    # Create dummy work dir for standalone testing
    dummy_work_dir = Path("temp_harness_work_dir")
    dummy_work_dir.mkdir(exist_ok=True)
    # Create dummy config and goal files if they don't exist
    if not Path("config.yaml").exists(): Path("config.yaml").touch()
    if not Path("goal.prompt").exists(): Path("goal.prompt").write_text("Create a hello world function.")

    harness = Harness(work_dir=dummy_work_dir)
    harness.run("Create a simple Python function that prints 'Hello, World!' and a test for it using pytest.")
    # Clean up dummy files/dirs after test run
    # import shutil
    # shutil.rmtree(dummy_work_dir)
    # Path("config.yaml").unlink(missing_ok=True)
    # Path("goal.prompt").unlink(missing_ok=True)
