from textual.app import App, ComposeResult
from textual.containers import Container
from textual.widgets import Header, Footer, Input, RichLog


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
        # You can access config values like self.config.get('ollama_model')

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
        self.query_one(Input).focus()
        log = self.query_one(RichLog)
        log.write("[bold green]Welcome to Veda TUI![/]")
        log.write("Enter your project goal or type commands.")
        # TODO: Add initial status information based on config/state

    async def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle user input submission."""
        user_input = event.value
        log = self.query_one(RichLog)
        input_widget = self.query_one(Input)

        if user_input:
            log.write(f"[bold blue]>>>[/] {user_input}")
            # TODO: Process the user input (e.g., send to chat logic, command parser)
            # For now, just echo it back or provide a placeholder response
            log.write(f"[yellow]Processing: '{user_input}'...[/] (Not implemented yet)")
            input_widget.clear()
            input_widget.focus() # Keep focus on input

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
