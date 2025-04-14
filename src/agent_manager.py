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


@dataclass
class AgentInstance:
    """Holds information about a running agent process."""
    role: str
    process: asyncio.subprocess.Process
    master_fd: Optional[int] = None # Master side of the pty
    read_task: Optional[asyncio.Task] = None
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
        # Default Aider model if not specified per role
        self.default_aider_model = config.get("aider_model")
        self.test_command = config.get("aider_test_command")
        self.agents: Dict[str, AgentInstance] = {} # role -> AgentInstance

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
        """Spawns a new Aider agent process."""
        if role in self.agents:
            logger.warning(f"Agent with role '{role}' already running.")
            # Optionally, post a message to the UI
            self.app.post_message(LogMessage(f"[orange3]Agent '{role}' is already running.[/]"))
            return

        agent_model = model or self.config.get(f"{role}_model") or self.default_aider_model
        if not agent_model:
            logger.error(f"No model specified for agent role '{role}' and no default aider_model configured.")
            self.app.post_message(LogMessage(f"[bold red]Error: No model configured for agent '{role}'.[/]"))
            return

        # Construct the command
        # Use shlex.split for basic safety, but be cautious with complex commands/paths
        command_parts = shlex.split(self.aider_command_base)
        command_parts.extend(["--model", agent_model])
        # Add other necessary aider args like git repo path, test command etc.
        # For now, just the model. Aider typically detects the git repo.
        if self.test_command:
             command_parts.extend(["--test-cmd", self.test_command])

        # TODO: Add logic to pass initial_prompt (maybe via stdin or a temp file?)
        # For now, we just start the agent.

        log_line = f"Spawning agent '{role}' with model '{agent_model}'..."
        logger.info(log_line)
        self.app.post_message(LogMessage(f"[yellow]{log_line}[/]"))

        master_fd, slave_fd = pty.openpty()

        # Make master_fd non-blocking
        fcntl.fcntl(master_fd, fcntl.F_SETFL, os.O_NONBLOCK)

        try:
            process = await asyncio.create_subprocess_exec(
                *command_parts,
                stdin=slave_fd,
                stdout=slave_fd,
                stderr=slave_fd,
                cwd=self.config.get("project_dir", "."), # Run aider in the project dir
                start_new_session=True # Important for pty
            )
            logger.info(f"Agent '{role}' spawned with PID {process.pid} using pty")

            # Close slave fd in parent process, it's not needed anymore
            os.close(slave_fd)

            # Create task to read combined output from the pty
            read_task = asyncio.create_task(self._read_pty_output(master_fd, role))

            # Store agent info
            self.agents[role] = AgentInstance(
                role=role, process=process, master_fd=master_fd, read_task=read_task
            )

            # Create a task to wait for the process to exit and post a message
            asyncio.create_task(self._monitor_agent_exit(role, process))

        except FileNotFoundError:
            os.close(master_fd) # Clean up master fd on error
            # os.close(slave_fd) # Already closed or handled by subprocess exec
            err_msg = f"Error: Command '{self.aider_command_base}' not found. Is Aider installed and in PATH?"
            logger.error(err_msg)
            self.app.post_message(LogMessage(f"[bold red]{err_msg}[/]"))
        except Exception as e:
            err_msg = f"Failed to spawn agent '{role}': {e}"
            logger.exception(err_msg)
            escaped_error = rich.markup.escape(str(e))
            self.app.post_message(LogMessage(f"[bold red]Failed to spawn agent '{role}': {escaped_error}[/]"))
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
        """Waits for an agent process to exit and posts a message."""
        return_code = await process.wait()
        logger.info(f"Agent '{role}' (PID {process.pid}) exited with code {return_code}")
        agent_instance = self.agents.get(role)
        self.app.post_message(AgentExitedMessage(role=role, return_code=return_code))
        # Clean up agent entry
        if agent_instance:
            # Cancel the reader task
            if agent_instance.read_task:
                agent_instance.read_task.cancel()
            # Close the master pty descriptor
            if agent_instance.master_fd is not None:
                try:
                    logger.info(f"Closing master_fd {agent_instance.master_fd} for agent '{role}'")
                    os.close(agent_instance.master_fd)
                except OSError as e:
                    logger.error(f"Error closing master_fd for agent '{role}': {e}")
            # Remove from tracking dict
            if role in self.agents:
                 del self.agents[role]


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
        """Sends data (e.g., user input) to the specified agent's pty."""
        agent_instance = self.agents.get(role)
        if not agent_instance or agent_instance.master_fd is None:
            logger.warning(f"Attempted to send data to non-existent or invalid agent '{role}'")
            return

        if not data.endswith('\n'):
            data += '\n' # Ensure input is terminated with newline for most CLIs

        try:
            logger.debug(f"Sending to agent '{role}' (fd {agent_instance.master_fd}): {data.strip()}")
            encoded_data = data.encode('utf-8')
            # Use os.write directly - consider loop.call_soon_threadsafe if called from worker
            bytes_written = os.write(agent_instance.master_fd, encoded_data)
            if bytes_written != len(encoded_data):
                 logger.warning(f"Short write to agent '{role}': wrote {bytes_written}/{len(encoded_data)} bytes")
        except OSError as e:
            logger.error(f"Error writing to pty for agent '{role}': {e}")
        except Exception as e:
            logger.exception(f"Unexpected error sending data to agent '{role}': {e}")


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
        Stops all managed agent processes gracefully.
        """
        logger.info(f"Stopping {len(self.agents)} agents...")
        for role, agent in list(self.agents.items()): # Iterate over a copy
            try:
                logger.info(f"Terminating agent '{role}' (PID {agent.process.pid})...")
                agent.process.terminate()
                # Wait briefly for termination, then kill if necessary
                await asyncio.wait_for(agent.process.wait(), timeout=5.0)
                logger.info(f"Agent '{role}' terminated.")
            except asyncio.TimeoutError:
                logger.warning(f"Agent '{role}' did not terminate gracefully, killing.")
                agent.process.kill()
            except ProcessLookupError:
                 logger.warning(f"Agent '{role}' process already exited.")
            except Exception as e:
                logger.exception(f"Error stopping agent '{role}': {e}")
            finally:
                # Cancel the reader task
                if agent.read_task:
                    agent.read_task.cancel()
                # Close the master pty descriptor
                if agent.master_fd is not None:
                    try:
                        logger.info(f"Closing master_fd {agent.master_fd} for agent '{role}' during stop_all")
                        os.close(agent.master_fd)
                    except OSError as e:
                        logger.error(f"Error closing master_fd for agent '{role}' during stop_all: {e}")
                # Remove from tracking - _monitor_agent_exit might also do this, but ensure removal
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
