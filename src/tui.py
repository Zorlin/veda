import logging
import rich.markup # Import for escaping
from textual.app import App, ComposeResult
from textual.containers import Container
from textual.widgets import Header, Footer, Input, RichLog
from pathlib import Path
from textual import work, message # Import message base class

from ollama_client import OllamaClient # Import the new client
from agent_manager import AgentManager # Import the AgentManager

# Configure logging for TUI
# Use a file for more persistent logs during development
logging.basicConfig(filename='veda_tui.log', level=logging.INFO, format='%(asctime)s - %(name)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


# --- Custom Messages for Worker -> UI Communication ---
class LogMessage(message.Message):
    """Custom message to log text to the RichLog."""
    def __init__(self, text: str) -> None:
        self.text = text
        super().__init__()

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

        # Initialize Agent Manager
        try:
            self.agent_manager = AgentManager(config=self.config, work_dir=self.work_dir)
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
        with Container(id="main-content"):
            with Container(id="log-container"):
                yield RichLog(id="main-log", wrap=True, highlight=True, markup=True)
            with Container(id="input-container"):
                yield Input(placeholder="Enter your command or message...", id="main-input")
        yield Footer()

    def on_mount(self) -> None:
        """Called when the app is mounted."""
        self.log_widget = self.query_one(RichLog)
        self.input_widget = self.query_one(Input)
        self.input_widget.focus()

        self.log_widget.write("[bold green]Welcome to Veda TUI![/]")
        if self.ollama_client:
            self.log_widget.write(f"Using Ollama model: [cyan]{self.ollama_client.model}[/]")
            # Trigger the initial prompt generation
            self.ask_initial_prompt()
        else:
            self.log_widget.write("[bold red]Error: Ollama client not initialized. Check config and logs.[/]")
            self.log_widget.write("Interaction disabled.")
            self.input_widget.disabled = True # Disable input if client failed

        # TODO: Add other initial status information based on config/state

    @work(exclusive=True, thread=True)
    def ask_initial_prompt(self) -> None:
        """Worker method to ask the initial user prompt using Ollama."""
        if not self.ollama_client:
            self.post_message(LogMessage("[bold red]Cannot generate initial prompt: Ollama client not available.[/]"))
            return

        initial_question = "What project goal should I work on today?"
        self.post_message(LogMessage("[italic grey50]Veda is thinking about the first question...[/]"))
        try:
            # Optional: Could ask Ollama to phrase the initial question, but let's keep it simple for now.
            # response = self.ollama_client.generate("Ask the user what project goal they want to work on.")
            # self.call_from_thread(self.log_widget.write, f"[bold magenta]Veda:[/bold] {response}")

            # Restore original markup, assuming escaping user input/LLM output fixes issues
            # Simplify markup first to diagnose
            self.post_message(LogMessage(f"[bold]Veda:[/bold] {initial_question}"))
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
            self.post_message(LogMessage("[bold red]Cannot process: Ollama client not available.[/]"))
            return

        # Post message for UI updates from worker
        self.post_message(LogMessage("[italic grey50]Thinking...[/]"))
        try:
            # Synchronous call within the worker thread
            response = self.ollama_client.generate(prompt)
            # Update UI from the worker thread safely
            self.post_message(LogMessage(f"[bold magenta]Veda ({self.ollama_client.model}):[/] {response}"))
        except Exception as e:
            # Log the exception and display an error in the TUI
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
            self.log_widget.write(f"[bold blue]>>> Project Goal:[/bold] {escaped_user_input}")
            if self.agent_manager:
                self.log_widget.write("[yellow]Initializing project orchestration...[/]")
                # Pass goal to AgentManager (runs in background, no worker needed here for now)
                # In the future, this might trigger async tasks within AgentManager
                try:
                    self.agent_manager.initialize_project(user_input)
                    self.project_goal_set = True
                    self.log_widget.write("[green]Project goal received. Veda will start working.[/]")
                    self.log_widget.write("You can provide further instructions or ask questions.")
                    self.input_widget.placeholder = "Enter further instructions or commands..."
                except Exception as e:
                    logger.exception("Error during project initialization")
                    escaped_error = rich.markup.escape(str(e))
                    self.log_widget.write(f"[bold red]Error initializing project: {escaped_error}[/]")
            else:
                self.log_widget.write("[bold red]Error: Agent Manager not available.[/]")
            self.input_widget.clear()
            self.input_widget.focus()

        elif self.ollama_client and not self.input_widget.disabled:
            # Subsequent input: Treat as chat/command for Veda's Ollama client
            escaped_user_input = rich.markup.escape(user_input)
            self.log_widget.write(f"[bold blue]>>>[/] {escaped_user_input}")
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
        """Handles logging text to the RichLog."""
        if self.log_widget:
            self.log_widget.write(message.text)

    def on_clear_input(self, message: ClearInput) -> None:
        """Handles clearing the input widget."""
        if self.input_widget:
            self.input_widget.clear()

    def on_focus_input(self, message: FocusInput) -> None:
        """Handles focusing the input widget."""
        if self.input_widget:
            self.input_widget.focus()
    # --- End Custom Message Handlers ---


    # No longer needed for synchronous client
    # async def on_unmount(self) -> None:
    #     """Called when the app is about to unmount."""
    #     if self.ollama_client:
    #         await self.ollama_client.close() # Gracefully close the client

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
