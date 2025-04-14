from textual.app import App, ComposeResult
from textual.widgets import Header, Footer, Static


class VedaApp(App):
    """The main Textual application for Veda."""

    BINDINGS = [
        ("d", "toggle_dark", "Toggle dark mode"),
        ("q", "quit", "Quit"),
    ]

    def __init__(self, config: dict, **kwargs):
        super().__init__(**kwargs)
        self.config = config
        # You can access config values like self.config.get('ollama_model')

    def compose(self) -> ComposeResult:
        """Create child widgets for the app."""
        yield Header()
        yield Static("Welcome to Veda TUI!", id="main-content")
        # TODO: Add more sophisticated layout and widgets based on requirements
        yield Footer()

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
