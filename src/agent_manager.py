import asyncio
import logging
import os
import pty
import fcntl
import shlex
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Optional

from textual import work
from textual.app import App
from textual.message import Message
import rich.markup # Import for escaping

# Import LogMessage from tui (or define it here if preferred)
# Assuming it's better defined alongside other messages if it becomes more complex,
# but for now, import from where it's used.
# If this causes circular import issues later, we'll move message definitions.
try:
    from tui import LogMessage
except ImportError:
    # Fallback if run standalone or during certain test setups
    @dataclass
    class LogMessage(Message):
        """Custom message to log text to the RichLog."""
        def __init__(self, text: str) -> None:
            self.text = text
            super().__init__()


logger = logging.getLogger(__name__)

# --- Custom Messages ---
@dataclass
class AgentOutputMessage(Message):
    """Message containing output from an agent process."""
    role: str
    line: str

@dataclass
class AgentExitedMessage(Message):
    """Message indicating an agent process has exited."""
    role: str
    return_code: Optional[int]
# --- End Custom Messages ---

# Forward declaration for type hinting
class OllamaClient:
    pass

@dataclass
class AgentInstance:
    """Holds information about a running agent process or client."""
    role: str
    agent_type: str # "aider" or "ollama"
    # For Aider agents (pty subprocess)
    process: Optional[asyncio.subprocess.Process] = None
    master_fd: Optional[int] = None # Master side of the pty
    read_task: Optional[asyncio.Task] = None
    # For Ollama agents (direct client)
    ollama_client: Optional[OllamaClient] = None
    # TODO: Add state, current task file, etc.


class AgentManager:
    """
    Manages the lifecycle and coordination of Aider agents.
    """
    def __init__(self, app: App, config: Dict, work_dir: Path):
        """
        Initializes the AgentManager.

        Args:
            app: The Textual App instance for posting messages.
            config: The application configuration dictionary.
            work_dir: The path to the working directory for agent communication.
        """
        self.app = app
        self.config = config
        self.work_dir = work_dir
        self.aider_command_base = config.get("aider_command", "aider")
        self.aider_model = config.get("aider_model") # Model specifically for aider agents
        self.test_command = config.get("aider_test_command")
        self.agents: Dict[str, AgentInstance] = {} # role -> AgentInstance

        # Define roles that use direct Ollama interaction
        self.ollama_roles = {
            "theorist", "architect", "skeptic", "historian", "coordinator", "planner",
            "arbiter", "canonizer", "redactor" # Add council roles if they interact directly
        }
        # Add code_reviewer if enabled and configured for direct ollama
        if config.get("enable_code_review") and config.get("code_review_model"):
             # Assuming direct ollama if a specific model is set, otherwise it might use aider?
             # Let's refine this logic if needed. For now, assume specific model means direct ollama.
             self.ollama_roles.add("code_reviewer")


        # Ensure work_dir exists
        self.work_dir.mkdir(parents=True, exist_ok=True)

        logger.info(f"AgentManager initialized. Work directory: {self.work_dir}")

    async def _read_pty_output(self, master_fd: int, role: str):
        """Reads output from the agent's pty and posts messages."""
        logger.info(f"Starting pty reader for agent '{role}' on fd {master_fd}")
        buffer = b""
        while True:
            try:
                # Use asyncio's event loop to wait for the fd to be readable
                # This avoids blocking the main thread with os.read
                loop = asyncio.get_running_loop()
                # Wait for the fd to be readable without blocking indefinitely
                # We might need a small timeout or check process status
                # Let's try reading directly first, handling BlockingIOError
                await asyncio.sleep(0.01) # Small sleep to prevent tight loop if no data
                chunk = os.read(master_fd, 1024) # Read up to 1KB
                if not chunk:
                    logger.info(f"EOF received from pty for agent '{role}'")
                    break # EOF

                buffer += chunk
                # Process lines
                while b'\n' in buffer:
                    line_bytes, buffer = buffer.split(b'\n', 1)
                    line = line_bytes.decode('utf-8', errors='replace').rstrip('\r') # rstrip for \r\n
                    if line: # Avoid posting empty lines
                        self.app.post_message(AgentOutputMessage(role=role, line=line))

            except BlockingIOError:
                # No data available right now, wait a bit
                await asyncio.sleep(0.05)
                continue
            except OSError as e:
                # This might happen if the fd is closed unexpectedly
                logger.error(f"OSError reading from pty for agent '{role}': {e}")
                break
            except asyncio.CancelledError:
                 logger.info(f"PTY reader task for agent '{role}' cancelled.")
                 break
            except Exception as e:
                logger.exception(f"Unexpected error reading pty for agent '{role}': {e}")
                break

        # Process any remaining buffer content after EOF
        if buffer:
            line = buffer.decode('utf-8', errors='replace').rstrip('\r')
            if line:
                self.app.post_message(AgentOutputMessage(role=role, line=line))

        logger.info(f"PTY reader task finished for agent '{role}'")
        # Note: Closing the master_fd is handled by _monitor_agent_exit or stop_all_agents

    async def spawn_agent(self, role: str, model: Optional[str] = None, initial_prompt: Optional[str] = None):
        """Spawns a new agent process (aider) or initializes a client (ollama)."""
        if role in self.agents:
            logger.warning(f"Agent with role '{role}' already running.")
            self.app.post_message(LogMessage(f"[orange3]Agent '{role}' is already running.[/]"))
            return

        # Determine agent type and model
        agent_type = "ollama" if role in self.ollama_roles else "aider"
        if agent_type == "ollama":
            # Use the specific model defined for this role, fallback to general ollama_model
            agent_model = self.config.get(f"{role}_model") or self.config.get("ollama_model")
            if not agent_model:
                 logger.error(f"No model specified for Ollama agent role '{role}' and no default ollama_model configured.")
                 self.app.post_message(LogMessage(f"[bold red]Error: No model configured for Ollama agent '{role}'.[/]"))
                 return
            log_line = f"Initializing Ollama client for agent '{role}' with model '{agent_model}'..."
            logger.info(log_line)
            self.app.post_message(LogMessage(f"[cyan]{log_line}[/]"))
            try:
                # Import late to avoid circular dependency if OllamaClient uses AgentManager types
                from ollama_client import OllamaClient
                client = OllamaClient(
                    api_url=self.config.get("ollama_api_url"),
                    model=agent_model,
                    timeout=self.config.get("ollama_request_timeout", 300),
                    options=self.config.get("ollama_options") # Use general ollama options for now
                )
                # Store agent info
                self.agents[role] = AgentInstance(
                    role=role, agent_type=agent_type, ollama_client=client
                )
                logger.info(f"Ollama client for agent '{role}' initialized.")
                # Post initial prompt if provided (needs handling in send_to_agent)
                if initial_prompt:
                    # We need to trigger the async send operation
                    asyncio.create_task(self.send_to_agent(role, initial_prompt))

            except Exception as e:
                 err_msg = f"Failed to initialize Ollama client for agent '{role}': {e}"
                 logger.exception(err_msg)
                 escaped_error = rich.markup.escape(str(e))
                 self.app.post_message(LogMessage(f"[bold red]{err_msg}[/]"))

        else: # agent_type == "aider"
            # Use the dedicated aider_model from config
            agent_model = self.aider_model
            if not agent_model:
                logger.error(f"No aider_model specified in config for Aider agent role '{role}'.")
                self.app.post_message(LogMessage(f"[bold red]Error: No aider_model configured for agent '{role}'.[/]"))
                return

            # Construct the aider command
            command_parts = shlex.split(self.aider_command_base)
            command_parts.extend(["--model", agent_model])
            if self.test_command:
                 command_parts.extend(["--test-cmd", self.test_command])
            # Add --no-show-model-warnings flag suggested by aider output
            command_parts.append("--no-show-model-warnings")

            # TODO: Pass initial_prompt to aider (maybe via stdin after start?)

            log_line = f"Spawning Aider agent '{role}' with model '{agent_model}'..."
            logger.info(log_line)
            self.app.post_message(LogMessage(f"[yellow]{log_line}[/]"))

            master_fd, slave_fd = pty.openpty()
            fcntl.fcntl(master_fd, fcntl.F_SETFL, os.O_NONBLOCK)

            try:
                process = await asyncio.create_subprocess_exec(
                    *command_parts,
                    stdin=slave_fd,
                    stdout=slave_fd,
                    stderr=slave_fd,
                    cwd=self.config.get("project_dir", "."),
                    start_new_session=True
                )
                logger.info(f"Aider agent '{role}' spawned with PID {process.pid} using pty")
                os.close(slave_fd)
                read_task = asyncio.create_task(self._read_pty_output(master_fd, role))
                self.agents[role] = AgentInstance(
                    role=role, agent_type=agent_type, process=process,
                    master_fd=master_fd, read_task=read_task
                )
                asyncio.create_task(self._monitor_agent_exit(role, process))
                # Send initial prompt if provided, after a short delay for aider to start
                if initial_prompt:
                    await asyncio.sleep(1.0) # Give aider a moment to start up
                    await self.send_to_agent(role, initial_prompt)


            except FileNotFoundError:
                os.close(master_fd)
                err_msg = f"Error: Command '{self.aider_command_base}' not found. Is Aider installed and in PATH?"
                logger.error(err_msg) # Correct indentation
                self.app.post_message(LogMessage(f"[bold red]{err_msg}[/]")) # Correct indentation
            except Exception as e:
                err_msg = f"Failed to spawn agent '{role}': {e}"
                logger.exception(err_msg)
                escaped_error = rich.markup.escape(str(e)) # Correct indentation
                self.app.post_message(LogMessage(f"[bold red]Failed to spawn agent '{role}': {escaped_error}[/]")) # Correct indentation
            # Ensure master_fd is closed if already created before exception
            if 'master_fd' in locals() and master_fd is not None:
                 try:
                     os.close(master_fd)
                 except OSError:
                     pass # Ignore if already closed
            # Slave might be open if error occurred after pty.openpty but before exec
            if 'slave_fd' in locals() and slave_fd is not None:
                 try:
                     os.close(slave_fd)
                 except OSError:
                     pass # Ignore if already closed

    async def _monitor_agent_exit(self, role: str, process: asyncio.subprocess.Process):
        """Waits for an Aider agent process to exit and posts a message."""
        # This monitor only applies to subprocesses (aider agents)
        return_code = await process.wait()
        logger.info(f"Aider agent '{role}' (PID {process.pid}) exited with code {return_code}")

        agent_instance = self.agents.get(role)
        # Ensure we only clean up if it's still the same process instance we were monitoring
        if agent_instance and agent_instance.process == process:
            self.app.post_message(AgentExitedMessage(role=role, return_code=return_code))
            # Clean up agent entry
            # Cancel the reader task
            if agent_instance.read_task:
                agent_instance.read_task.cancel()
            # Close the master pty descriptor
            if agent_instance.master_fd is not None:
                try:
                    logger.info(f"Closing master_fd {agent_instance.master_fd} for agent '{role}' on exit")
                    os.close(agent_instance.master_fd)
                except OSError as e:
                    # May already be closed by stop_all_agents, ignore EBADF
                    if e.errno != 9: # errno 9 is EBADF (Bad file descriptor)
                         logger.error(f"Error closing master_fd for agent '{role}' on exit: {e}")
            # Remove from tracking dict
            if role in self.agents:
                 del self.agents[role]
        else:
             logger.warning(f"Monitor task for agent '{role}' found inconsistent state or agent already removed.")


    async def initialize_project(self, project_goal: str):
        """
        Starts the process based on the user's project goal.

        Args:
            project_goal: The initial goal provided by the user.
        """
        log_line = f"Received project goal: '{project_goal}'"
        logger.info(log_line)
        self.app.post_message(LogMessage(f"[green]{log_line}[/]")) # Use LogMessage for status
        logger.info(f"Work directory is: {self.work_dir.resolve()}")

        # Example: Write initial goal to a file in workdir
        try:
            goal_file = self.work_dir / "initial_goal.txt"
            with open(goal_file, "w") as f:
                f.write(project_goal)
            logger.info(f"Initial goal written to {goal_file}")
            self.app.post_message(LogMessage(f"Initial goal saved to {goal_file.name}"))
        except IOError as e:
            err_msg = f"Failed to write initial goal to {goal_file}: {e}"
            logger.error(err_msg)
            self.app.post_message(LogMessage(f"[bold red]{err_msg}[/]"))

        # --- Spawn the initial agent ---
        # Determine initial agent role and model (e.g., planner/coordinator)
        # Using coordinator_model as a placeholder for the initial planner/analyzer
        initial_agent_role = "planner"
        initial_agent_model = self.config.get("coordinator_model") # Or another designated planner model

        if initial_agent_model:
            # Pass the project goal as the initial prompt/task for the agent
            await self.spawn_agent(
                role=initial_agent_role,
                model=initial_agent_model,
                initial_prompt=project_goal # TODO: Pass this prompt effectively
            )
        else:
            err_msg = "Coordinator model not defined in config, cannot start initial agent."
            logger.error(err_msg)
            self.app.post_message(LogMessage(f"[bold red]{err_msg}[/]"))
        # -----------------------------

    async def send_to_agent(self, role: str, data: str):
        """Sends data (e.g., user input) to the specified agent."""
        agent_instance = self.agents.get(role)
        if not agent_instance:
            logger.warning(f"Attempted to send data to non-existent agent '{role}'")
            return

        if agent_instance.agent_type == "ollama":
            if agent_instance.ollama_client:
                # Run Ollama call in a worker thread
                logger.info(f"Sending prompt to Ollama agent '{role}': {data[:100]}...")
                # Post a message indicating the agent is thinking
                self.app.post_message(LogMessage(f"[italic grey50]Agent '{role}' is thinking...[/]"))
                # Call the static worker method via the app instance
                self.app.run_worker(
                    self._call_ollama_agent_worker(
                        app=self.app,
                        client=agent_instance.ollama_client,
                        role=role,
                        prompt=data
                    ),
                    exclusive=True
                )
            else:
                logger.error(f"Ollama agent '{role}' has no client instance.")
                self.app.post_message(LogMessage(f"[bold red]Error: Ollama agent '{role}' not properly initialized.[/]"))

        elif agent_instance.agent_type == "aider":
            if agent_instance.master_fd is None:
                 logger.warning(f"Attempted to send data to Aider agent '{role}' with no valid pty")
                 return

            if not data.endswith('\n'):
                data += '\n' # Ensure input is terminated with newline for most CLIs

            try:
                logger.debug(f"Sending to Aider agent '{role}' (fd {agent_instance.master_fd}): {data.strip()}")
                encoded_data = data.encode('utf-8')
                bytes_written = os.write(agent_instance.master_fd, encoded_data)
                if bytes_written != len(encoded_data):
                     logger.warning(f"Short write to Aider agent '{role}': wrote {bytes_written}/{len(encoded_data)} bytes")
            except OSError as e:
                logger.error(f"Error writing to pty for Aider agent '{role}': {e}")
                # Maybe post an error message or try to handle agent exit?
            except Exception as e:
                logger.exception(f"Unexpected error sending data to Aider agent '{role}': {e}")
        else:
             logger.error(f"Unknown agent type '{agent_instance.agent_type}' for role '{role}'")

    # Make this a static method as it's called by the App's run_worker
    # It doesn't need 'self' (AgentManager instance)
    @staticmethod
    @work(exclusive=True, thread=True)
    def _call_ollama_agent_worker(app: App, client: OllamaClient, role: str, prompt: str):
        """Worker thread function to call the Ollama client for a specific agent."""
        if not client:
             logger.error(f"No Ollama client provided for agent '{role}' in worker.")
             # Post error message back to the app
             app.post_message(AgentOutputMessage(role=role, line="[bold red]Error: Ollama client missing in worker.[/]"))
             return

        try:
            logger.info(f"Ollama worker started for agent '{role}'.")
            response = client.generate(prompt)
            # Post the response back to the UI, attributed to the agent
            app.post_message(AgentOutputMessage(role=role, line=response))
        except Exception as e:
            logger.exception(f"Error during Ollama call for agent '{role}':")
            escaped_error = rich.markup.escape(str(e))
            # Post error message attributed to the agent
            app.post_message(AgentOutputMessage(role=role, line=f"[bold red]Error: {escaped_error}[/]"))
        finally:
             # Maybe focus input or indicate completion? Depends on workflow.
             # For now, just log completion.
             logger.info(f"Ollama call finished for agent '{role}'")


    async def manage_agents(self):
        """
        The main loop or method to monitor and manage running agents.
        (Placeholder for future implementation)
        """
        # TODO: Monitor workdir for agent handoffs, status updates, errors.
        # TODO: Spawn new agents as needed based on handoff files.
        # TODO: Report progress/status back to the UI via messages.
        await asyncio.sleep(1) # Placeholder to prevent busy-loop if called repeatedly

    async def stop_all_agents(self):
        """
        Stops all managed agent processes/clients gracefully.
        """
        logger.info(f"Stopping {len(self.agents)} agents...")
        agent_roles = list(self.agents.keys()) # Get roles before iterating/deleting

        for role in agent_roles:
            agent = self.agents.get(role)
            if not agent:
                continue # Agent might have exited and been removed already

            try:
                if agent.agent_type == "aider" and agent.process:
                    logger.info(f"Terminating Aider agent '{role}' (PID {agent.process.pid})...")
                    if agent.process.returncode is None: # Only terminate if running
                        agent.process.terminate()
                        # Wait briefly for termination, then kill if necessary
                        await asyncio.wait_for(agent.process.wait(), timeout=5.0)
                        logger.info(f"Aider agent '{role}' terminated.")
                    else:
                         logger.info(f"Aider agent '{role}' already exited with code {agent.process.returncode}.")

                elif agent.agent_type == "ollama":
                    # Ollama clients don't need explicit stopping currently
                    logger.info(f"Stopping Ollama agent '{role}' (no process to terminate).")
                    pass # No process to stop

            except asyncio.TimeoutError:
                if agent.agent_type == "aider" and agent.process:
                    logger.warning(f"Aider agent '{role}' did not terminate gracefully, killing.")
                    if agent.process.returncode is None:
                         agent.process.kill()
            except ProcessLookupError:
                 logger.warning(f"Aider agent '{role}' process already exited.")
            except Exception as e:
                logger.exception(f"Error stopping agent '{role}': {e}")
            finally:
                # Cleanup resources regardless of agent type or errors
                if agent.read_task: # Cancel reader task if it exists (aider)
                    agent.read_task.cancel()
                if agent.master_fd is not None: # Close pty fd if it exists (aider)
                    try:
                        logger.info(f"Closing master_fd {agent.master_fd} for agent '{role}' during stop_all")
                        os.close(agent.master_fd)
                    except OSError as e:
                         # Ignore EBADF as it might be closed by _monitor_agent_exit already
                         if e.errno != 9:
                             logger.error(f"Error closing master_fd for agent '{role}' during stop_all: {e}")
                # Remove from tracking dict
                if role in self.agents:
                    del self.agents[role]
        logger.info("Finished stopping agents.")

# Example usage (optional, for testing)
# if __name__ == "__main__":
#     test_cfg = {
#         "aider_command": "aider",
#         "aider_model": "gpt-4",
#         "aider_test_command": "pytest -v"
#     }
#     workdir = Path("./test_workdir")
#     manager = AgentManager(config=test_cfg, work_dir=workdir)
#     manager.initialize_project("Create a simple Flask web server.")
#     # manager.stop_all_agents() # Example cleanup
#     # import shutil
#     # if workdir.exists():
#     #     shutil.rmtree(workdir)
