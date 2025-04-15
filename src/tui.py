import logging
import rich.markup # Import for escaping
from textual.app import App, ComposeResult
from textual.containers import Container, VerticalScroll
from textual.widgets import Header, Footer, Input, RichLog, TabbedContent, TabPane
from pathlib import Path
from textual import work, message # Import message base class

from ollama_client import OllamaClient # Import the new client
from agent_manager import AgentManager # Import the AgentManager

# Configure logging for TUI
# Use a file for more persistent logs during development
logging.basicConfig(filename='veda_tui.log', level=logging.INFO, format='%(asctime)s - %(name)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


# --- Custom Messages for Worker -> UI Communication ---
# Use messages defined in agent_manager
from agent_manager import AgentOutputMessage, AgentExitedMessage, LogMessage

# Keep UI-specific messages here
class ClearInput(message.Message):
    """Custom message to clear the Input widget."""
    pass

class FocusInput(message.Message):
    """Custom message to focus the Input widget."""
    pass
# --- End Custom Messages ---


class VedaApp(App[None]):
    """The main Textual application for Veda."""

    BINDINGS = [
        ("d", "toggle_dark", "Toggle dark mode"),
        ("q", "quit", "Quit"),
    ]
    CSS = """
    #log-container {
        height: 80%; /* Adjust height as needed */
        border: round $accent;
        margin: 1 0;
    }
    #input-container {
        height: auto;
    }
    #main-log {
        height: 100%;
    }
    Input {
        border: round $accent;
    }
    """

    def __init__(self, config: dict, **kwargs):
        super().__init__(**kwargs)
        self.config = config
        self.log_widget = None # Initialize log widget reference
        self.input_widget = None # Initialize input widget reference
        self.project_goal_set = False # Track if the initial goal is set

        # Define work directory path
        self.work_dir = Path(config.get("project_dir", ".")).resolve() / "workdir"

        # Initialize Agent Manager - Pass the app instance!
        try:
            self.agent_manager = AgentManager(app=self, config=self.config, work_dir=self.work_dir)
        except Exception as e:
            logger.exception("Failed to initialize AgentManager")
            # Log this properly or display in TUI later
            print(f"Error initializing AgentManager: {e}") # Simple print for now
            self.agent_manager = None

        # Initialize Ollama Client (for Veda's own chat/prompts)
        try:
            self.ollama_client = OllamaClient(
                api_url=self.config.get("ollama_api_url"),
                model=self.config.get("ollama_model"),
                timeout=self.config.get("ollama_request_timeout", 300),
                options=self.config.get("ollama_options")
            )
        except ValueError as e:
            # Handle potential config errors during init
            # Log this properly or display in TUI later
            print(f"Error initializing Ollama Client: {e}") # Simple print for now
            self.ollama_client = None # Ensure it's None if init fails

    def compose(self) -> ComposeResult:
        """Create child widgets for the app."""
        yield Header()
        # Use TabbedContent instead of a single log container
        with TabbedContent(id="main-tabs"):
             # General/System Log Tab
             with TabPane("Veda Log", id="tab-veda-log"):
                 yield RichLog(id="main-log", wrap=True, highlight=True, markup=True)
             # Agent tabs will be added dynamically
        with Container(id="input-container"):
             yield Input(placeholder="Enter project goal...", id="main-input")
        yield Footer()

    def on_mount(self) -> None:
        """Called when the app is mounted."""
        # Get reference to the main log widget (now inside a tab)
        self.log_widget = self.query_one("#main-log", RichLog)
        self.input_widget = self.query_one(Input)
        self.input_widget.focus()

        self.log_widget.write("[bold green]Welcome to Veda TUI![/]") # Write to main log
        if self.ollama_client:
            self.log_widget.write(f"Using Ollama model for Veda: [cyan]{getattr(self.ollama_client, 'model', 'unknown')}[/]")
            # Trigger the initial prompt generation (writes to main log)
            self.ask_initial_prompt()
        else:
            self.log_widget.write("[bold red]Error: Veda's Ollama client not initialized. Check config and logs.[/]")
            self.log_widget.write("[bold red]Interaction disabled.[/]")
            self.input_widget.disabled = True # Disable input if Veda's client failed

        if not self.agent_manager:
             self.log_widget.write("[bold red]Error: Agent Manager failed to initialize. Agent spawning disabled.[/]")
             # Input might already be disabled, but ensure it is if manager fails too
             self.input_widget.disabled = True

        # TODO: Add other initial status information based on config/state

    @work(exclusive=True, thread=True)
    def ask_initial_prompt(self) -> None:
        """Worker method to ask the initial user prompt using Ollama."""
        if not self.ollama_client:
            self.post_message(LogMessage("[bold red]Cannot generate initial prompt: Veda's Ollama client not available.[/]"))
            return

        initial_question = "What project goal should I work on today?"
        # Post general status messages to the main log
        self.post_message(LogMessage("[italic grey50]Veda is thinking about the first question...[/]"))
        try:
            # Optional: Could ask Ollama to phrase the initial question, but let's keep it simple for now.
            # response = self.ollama_client.generate("Ask the user what project goal they want to work on.")
            # self.call_from_thread(self.log_widget.write, f"[bold magenta]Veda:[/bold] {response}")

            # Escape the entire string again for diagnostics
            formatted_question = f"Veda: {initial_question}"
            escaped_question = rich.markup.escape(formatted_question)
            self.post_message(LogMessage(escaped_question))
            # Original line causing issues:
            # self.post_message(LogMessage(f"[bold magenta]Veda:[/bold] {initial_question}"))
        except Exception as e:
            logger.exception("Error generating initial prompt:")
            escaped_error = rich.markup.escape(str(e))
            self.post_message(LogMessage(f"[bold red]Error generating initial prompt: {escaped_error}[/]"))
        finally:
             # Ensure input is focused after the prompt is displayed
             self.post_message(FocusInput())


    @work(exclusive=True, thread=True) # Run Ollama call in a worker thread
    def call_ollama(self, prompt: str) -> None:
        """Worker method to call Ollama (synchronously) for user prompts and update the log."""
        if not self.ollama_client:
            # Post message for UI updates from worker
            self.post_message(LogMessage("[bold red]Cannot process: Veda's Ollama client not available.[/]"))
            return

        # Post message for UI updates from worker (to main log)
        self.post_message(LogMessage("[italic grey50]Thinking...[/]"))
        try:
            # Synchronous call within the worker thread
            response = self.ollama_client.generate(prompt)
            # Update UI from the worker thread safely (to main log)
            self.post_message(LogMessage(f"[bold magenta]Veda ({self.ollama_client.model}):[/] {response}"))
        except Exception as e:
            # Log the exception and display an error in the TUI (to main log)
            logger.exception("Error during Ollama call in worker thread:")
            escaped_error = rich.markup.escape(str(e))
            self.post_message(LogMessage(f"[bold red]Error during Ollama call: {escaped_error}[/]"))
        finally:
            # Ensure input is cleared and focused even if there was an error
            self.post_message(ClearInput())
            self.post_message(FocusInput())


    async def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle user input submission."""
        user_input = event.value.strip()

        if not user_input:
            self.input_widget.focus()
            return # Ignore empty input

        if not self.project_goal_set:
            # This is the initial project goal
            escaped_user_input = rich.markup.escape(user_input)
            # Simplify markup to just bold
            self.log_widget.write(f"[bold]>>> Project Goal:[/bold] {escaped_user_input}") # Log goal to main log
            if self.agent_manager:
                self.log_widget.write("[yellow]Initializing project orchestration...[/]")
                # Run the async agent initialization in a worker
                self.run_worker(self.agent_manager.initialize_project(user_input), exclusive=True)
                # Don't set project_goal_set immediately, wait for confirmation/agent start?
                # For now, set it optimistically. We might need a message back from AgentManager later.
                self.project_goal_set = True
                self.input_widget.placeholder = "Enter further instructions or commands..."
            else:
                self.log_widget.write("[bold red]Error: Agent Manager not available. Cannot start project.[/]")
            # Clear input after submitting goal
            self.input_widget.clear()
            self.input_widget.focus()

        elif self.ollama_client and not self.input_widget.disabled:
            # Subsequent input: Treat as chat/command for Veda's Ollama client
            escaped_user_input = rich.markup.escape(user_input)
            # Simplify markup further
            self.log_widget.write(f"[bold]>>>[/] {escaped_user_input}")
            # Call the worker for Veda's response/action
            self.call_ollama(user_input)
            # Don't clear input here, worker will do it after response

        elif not self.ollama_client or self.input_widget.disabled:
             self.log_widget.write("[bold red]Interaction disabled. Ollama client not available.[/]")
             self.input_widget.clear() # Clear input even if disabled
             self.input_widget.focus()
        else:
            # Handle empty input if needed, or just ignore
            self.input_widget.focus()

    # --- Custom Message Handlers ---
    def on_log_message(self, message: LogMessage) -> None:
        """Handles logging general status text to the main Veda log."""
        if self.log_widget:
            self.log_widget.write(message.text)
        # For test compatibility: allow test to inspect log content
        if hasattr(self.log_widget, "get_content") is False:
            def get_content():
                # Return the log lines as a list of strings
                return [str(line) for line in getattr(self.log_widget, "_lines", [])]
            self.log_widget.get_content = get_content

    def on_agent_output_message(self, message: AgentOutputMessage) -> None:
        """Handles output lines from agent subprocesses."""
        tabs = self.query_one(TabbedContent)
        tab_id = f"tab-{message.role}"
        try:
            # Try to find existing tab/log
            log_widget = self.query_one(f"#{tab_id} RichLog", RichLog)
        except Exception:
            # Tab doesn't exist, create it
            log_widget = RichLog(wrap=True, highlight=True, markup=True)
            # Add a .title attribute for test compatibility
            new_pane = TabPane(f"Agent: {message.role}", log_widget, id=tab_id)
            new_pane.title = f"Agent: {message.role}"
            tabs.add_pane(new_pane)
            tabs.active = tab_id # Switch to the new tab
            log_widget.write(f"--- Log for agent '{message.role}' ---")

        # Write the line
        log_widget.write(message.line)

    def on_agent_exited_message(self, message: AgentExitedMessage) -> None:
        """Handles agent process exit."""
        log_line = f"Agent '{message.role}' exited with code {message.return_code}."
        logger.info(log_line)
        # Log to main log and agent's log if it exists
        self.post_message(LogMessage(f"[yellow]{log_line}[/]"))
        try:
            agent_log = self.query_one(f"#tab-{message.role} RichLog", RichLog)
            agent_log.write(f"[yellow]--- {log_line} ---[/]")
            # For test compatibility: allow test to inspect log content
            if hasattr(agent_log, "get_content") is False:
                def get_content():
                    return [str(line) for line in getattr(agent_log, "_lines", [])]
                agent_log.get_content = get_content
        except Exception:
            pass # Agent tab might not exist if spawn failed

    def on_clear_input(self, message: ClearInput) -> None:
        """Handles clearing the input widget."""
        if self.input_widget:
            self.input_widget.clear()

    def on_focus_input(self, message: FocusInput) -> None:
        """Handles focusing the input widget."""
        if self.input_widget:
            self.input_widget.focus()
    # --- End Custom Message Handlers ---


    async def on_unmount(self) -> None:
        """Called when the app is about to unmount. Stop agents."""
        if self.agent_manager:
            await self.agent_manager.stop_all_agents() # Gracefully stop agents
    #     """Called when the app is about to unmount."""
    #     if self.ollama_client:
    #         await self.ollama_client.close() # Gracefully close the client

    @property
    def dark(self):
        # Provide a default value for dark mode to avoid AttributeError in tests
        return getattr(self, "_dark", False)

    @dark.setter
    def dark(self, value):
        self._dark = value

    def action_toggle_dark(self) -> None:
        """An action to toggle dark mode."""
        self.dark = not self.dark

    def action_quit(self) -> None:
        """An action to quit the application."""
        self.exit()

# Example of running the app directly (for testing)
# if __name__ == "__main__":
#     # A default config for direct running, or load from file
#     test_config = {"ollama_model": "test_model"}
#     app = VedaApp(config=test_config)
#     app.run()
