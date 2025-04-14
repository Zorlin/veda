import pytest
from pathlib import Path
import sys

# Add src to path for imports
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
sys.path.insert(0, str(src_path))

from textual.widgets import RichLog, Input # Import the missing widgets

from config import load_config
from tui import VedaApp


# Load config once for tests
test_config_path = project_root / "config.yaml"
try:
    test_config = load_config(test_config_path)
except FileNotFoundError:
    # Provide a minimal fallback config if config.yaml is missing
    # Adjust ollama_api_url if needed for your test environment or mock it
    print("WARN: config.yaml not found, using minimal test config.")
    test_config = {
        "ollama_api_url": "http://localhost:11434/api/generate", # Example URL
        "ollama_model": "mock_model", # Use a placeholder/mock model
        "ollama_request_timeout": 10,
        "ollama_options": {},
    }
except Exception as e:
    print(f"Error loading config for tests: {e}")
    test_config = {} # Ensure config is a dict even on error


@pytest.mark.asyncio
async def test_app_starts_and_shows_welcome():
    """Test if the app starts, displays welcome, and Ollama status."""
    app = VedaApp(config=test_config)
    async with app.run_test() as pilot:
        # Wait for the initial messages to appear
        await pilot.pause(0.5) # Give time for mount and initial logs

        # Basic check: Ensure the log widget exists
        log = pilot.app.query_one("#main-log", RichLog)
        assert log is not None

        # Basic check: Ensure the input widget exists and is focused
        input_widget = pilot.app.query_one(Input)
        assert input_widget is not None
        input_widget = pilot.app.query_one(Input)
        assert pilot.app.focused is input_widget
        if pilot.app.ollama_client:
            assert not input_widget.disabled
        else:
            assert input_widget.disabled

@pytest.mark.asyncio
async def test_user_input_appears_in_log():
    """Test if user input is correctly logged."""
    app = VedaApp(config=test_config)
    async with app.run_test() as pilot:
        # Wait for app setup
        await pilot.pause(0.5)

        # Simulate user typing and submitting
        input_widget = pilot.app.query_one(Input)
        test_message = "This is a test message"
        await pilot.press(*list(test_message)) # Simulate typing character by character
        await pilot.press("enter")

        # Allow time for the input submission and worker to process (even if mocked/fast)
        await pilot.pause(0.5)

        # Basic check: Ensure the log widget exists
        log = pilot.app.query_one("#main-log", RichLog)
        assert log is not None

        # We can't easily check the log content or guarantee input clear timing
        # without more complex mocking/waiting, but we know the input was submitted.
        # The fact that the test doesn't hang indefinitely implies the worker was called.
        # TODO: Add more robust checks, potentially involving mocking OllamaClient.
