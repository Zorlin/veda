import logging
import rich.markup # Import for escaping
from textual.app import App, ComposeResult
from textual.containers import Container
from textual.widgets import Header, Footer, Input, RichLog
from textual import work

from ollama_client import OllamaClient # Import the new client

# Configure logging for TUI
# Use a file for more persistent logs during development
logging.basicConfig(filename='veda_tui.log', level=logging.INFO, format='%(asctime)s - %(name)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


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

        # Initialize Ollama Client
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
            self.call_from_thread(self.log_widget.write, "[bold red]Cannot generate initial prompt: Ollama client not available.[/]")
            return

        initial_question = "What project goal should I work on today?"
        self.call_from_thread(self.log_widget.write, "[italic grey50]Veda is thinking about the first question...[/]")
        try:
            # Optional: Could ask Ollama to phrase the initial question, but let's keep it simple for now.
            # response = self.ollama_client.generate("Ask the user what project goal they want to work on.")
            # self.call_from_thread(self.log_widget.write, f"[bold magenta]Veda:[/bold] {response}")

            # Escape the entire string for now to diagnose MarkupError
            formatted_question = f"Veda: {initial_question}"
            escaped_question = rich.markup.escape(formatted_question)
            self.call_from_thread(self.log_widget.write, escaped_question)
            # Original line causing issues (potentially):
            # self.call_from_thread(self.log_widget.write, f"[bold magenta]Veda:[/bold] {initial_question}")
        except Exception as e:
            logger.exception("Error generating initial prompt:")
            escaped_error = rich.markup.escape(str(e))
            self.call_from_thread(self.log_widget.write, f"[bold red]Error generating initial prompt: {escaped_error}[/]")
        finally:
             # Ensure input is focused after the prompt is displayed
             self.call_from_thread(self.input_widget.focus)


    @work(exclusive=True, thread=True) # Run Ollama call in a worker thread
    def call_ollama(self, prompt: str) -> None:
        """Worker method to call Ollama (synchronously) for user prompts and update the log."""
        if not self.ollama_client:
            # Use call_from_thread for UI updates from worker
            self.call_from_thread(self.log_widget.write, "[bold red]Cannot process: Ollama client not available.[/]")
            return

        # Use call_from_thread for UI updates from worker
        self.call_from_thread(self.log_widget.write, "[italic grey50]Thinking...[/]")
        try:
            # Synchronous call within the worker thread
            response = self.ollama_client.generate(prompt)
            # Update UI from the worker thread safely
            self.call_from_thread(self.log_widget.write, f"[bold magenta]Veda ({self.ollama_client.model}):[/] {response}")
        except Exception as e:
            # Log the exception and display an error in the TUI
            logger.exception("Error during Ollama call in worker thread:")
            escaped_error = rich.markup.escape(str(e))
            self.call_from_thread(self.log_widget.write, f"[bold red]Error during Ollama call: {escaped_error}[/]")
        finally:
            # Ensure input is cleared and focused even if there was an error
            self.call_from_thread(self.input_widget.clear)
            self.call_from_thread(self.input_widget.focus)


    async def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle user input submission."""
        user_input = event.value

        if user_input and self.ollama_client and not self.input_widget.disabled:
            self.log_widget.write(f"[bold blue]>>>[/] {user_input}")
            # Call the worker
            self.call_ollama(user_input)
            # Don't clear input here, worker will do it after response
        elif not self.ollama_client or self.input_widget.disabled:
             self.log_widget.write("[bold red]Interaction disabled. Ollama client not available.[/]")
             self.input_widget.clear() # Clear input even if disabled
             self.input_widget.focus()
        else:
            # Handle empty input if needed, or just ignore
            self.input_widget.focus()

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
