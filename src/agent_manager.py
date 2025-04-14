import asyncio
import logging
import shlex
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Optional

from textual import work
from textual.app import App
from textual.message import Message

logger = logging.getLogger(__name__)

# --- Custom Messages ---
@dataclass
class AgentOutputMessage(Message):
    """Message containing output from an agent process."""
    role: str
    line: str
    is_stderr: bool = False

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
    stdout_task: Optional[asyncio.Task] = None
    stderr_task: Optional[asyncio.Task] = None
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

    async def _read_stream(self, stream: Optional[asyncio.StreamReader], role: str, is_stderr: bool):
        """Reads lines from a stream and posts them as messages."""
        if stream is None:
            return
        while True:
            line_bytes = await stream.readline()
            if not line_bytes:
                break
            line = line_bytes.decode('utf-8', errors='replace').rstrip()
            self.app.post_message(AgentOutputMessage(role=role, line=line, is_stderr=is_stderr))
        logger.info(f"{'Stderr' if is_stderr else 'Stdout'} stream closed for agent '{role}'")


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
        self.app.post_message(LogMessage(f"[yellow]{log_line}[/]")) # Use LogMessage for general status

        try:
            process = await asyncio.create_subprocess_exec(
                *command_parts,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                stdin=asyncio.subprocess.PIPE, # Keep stdin open if needed later
                cwd=self.config.get("project_dir", ".") # Run aider in the project dir
            )
            logger.info(f"Agent '{role}' spawned with PID {process.pid}")

            # Create tasks to read stdout and stderr
            stdout_task = asyncio.create_task(self._read_stream(process.stdout, role, is_stderr=False))
            stderr_task = asyncio.create_task(self._read_stream(process.stderr, role, is_stderr=True))

            # Store agent info
            self.agents[role] = AgentInstance(
                role=role, process=process, stdout_task=stdout_task, stderr_task=stderr_task
            )

            # Create a task to wait for the process to exit and post a message
            asyncio.create_task(self._monitor_agent_exit(role, process))

        except FileNotFoundError:
            err_msg = f"Error: Command '{self.aider_command_base}' not found. Is Aider installed and in PATH?"
            logger.error(err_msg)
            self.app.post_message(LogMessage(f"[bold red]{err_msg}[/]"))
        except Exception as e:
            err_msg = f"Failed to spawn agent '{role}': {e}"
            logger.exception(err_msg)
            escaped_error = rich.markup.escape(str(e))
            self.app.post_message(LogMessage(f"[bold red]Failed to spawn agent '{role}': {escaped_error}[/]"))

    async def _monitor_agent_exit(self, role: str, process: asyncio.subprocess.Process):
        """Waits for an agent process to exit and posts a message."""
        return_code = await process.wait()
        logger.info(f"Agent '{role}' (PID {process.pid}) exited with code {return_code}")
        self.app.post_message(AgentExitedMessage(role=role, return_code=return_code))
        # Clean up agent entry
        if role in self.agents:
            # Cancel reader tasks if they are still running (though they should end on EOF)
            if self.agents[role].stdout_task:
                self.agents[role].stdout_task.cancel()
            if self.agents[role].stderr_task:
                self.agents[role].stderr_task.cancel()
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
                # Cancel reader tasks
                if agent.stdout_task:
                    agent.stdout_task.cancel()
                if agent.stderr_task:
                    agent.stderr_task.cancel()
                # Remove from tracking - _monitor_agent_exit might also do this
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
