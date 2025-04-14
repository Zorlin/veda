import pytest
from pathlib import Path
import sys

# Add src to path for imports
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
sys.path.insert(0, str(src_path))

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

        log = pilot.app.query_one("#main-log", RichLog)
        log_text = log.to_plain_text()

        assert "Welcome to Veda TUI!" in log_text
        if pilot.app.ollama_client:
             assert f"Connected to Ollama model: {pilot.app.ollama_client.model}" in log_text
             # We won't test the *exact* initial prompt from Ollama here,
             # as it's dynamic, but we could check for "Thinking..." or similar later.
        else:
             assert "Error: Ollama client not initialized" in log_text

        # Ensure input is initially focused and enabled (if client is ok)
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

        log = pilot.app.query_one("#main-log", RichLog)
        log_text = log.to_plain_text()

        # Check if the user's input is logged
        assert f">>> {test_message}" in log_text
        # Depending on whether Ollama is mocked or live, check for "Thinking..." or response
        # For now, just checking the input log is sufficient
