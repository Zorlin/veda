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

        # Check if the input appears in the main log
        main_log_widget = pilot.app.query_one("#main-log", RichLog)
        assert ">>> This is a test message" in main_log_widget.export_text(strip_styles=True)

        # Check if the "thinking" message appeared (assuming Ollama client is mocked/available)
        # This might fail if the Ollama client init failed in the fixture
        if pilot.app.ollama_client:
            assert "Thinking..." in main_log_widget.export_text(strip_styles=True)
            # Check if the (mocked) response appeared - Requires mocking OllamaClient in the fixture
            # assert f"Veda ({test_config['ollama_model']}): Mock Ollama Response" in main_log_widget.export_text(strip_styles=True)

        # Check if input was cleared (happens in the worker's finally block)
        input_widget = pilot.app.query_one(Input)
        assert input_widget.value == ""

@pytest.mark.asyncio
async def test_agent_tab_creation_and_output(test_config):
            """Test that agent output creates a new tab and logs correctly."""
            app = VedaApp(config=test_config)
            async with app.run_test() as pilot:
                # Wait for initial prompt to finish if necessary
                await pilot.pause(0.1)

                # Simulate receiving output from a new agent
                agent_role = "coder"
                agent_line_1 = "Agent coder starting..."
                agent_line_2 = "```python\nprint('Hello from coder')\n```"
                message1 = AgentOutputMessage(role=agent_role, line=agent_line_1)
                message2 = AgentOutputMessage(role=agent_role, line=agent_line_2)

                # Post messages as if they came from the agent manager
                pilot.app.post_message(message1)
                await pilot.pause(0.1) # Allow UI to update
                pilot.app.post_message(message2)
                await pilot.pause(0.1) # Allow UI to update

                # Check if the new tab exists
                tabbed_content = pilot.app.query_one(TabbedContent)
                agent_tab_pane = pilot.app.query_one(f"#tab-{agent_role}", TabPane)
                assert agent_tab_pane is not None
                assert agent_tab_pane.title == f"Agent: {agent_role}"

                # Check if the log widget within the tab exists and contains the output
                agent_log_widget = agent_tab_pane.query_one(RichLog)
                log_text = agent_log_widget.export_text(strip_styles=True)
                assert f"--- Log for agent '{agent_role}' ---" in log_text
                assert agent_line_1 in log_text
                assert agent_line_2 in log_text

        @pytest.mark.asyncio
        async def test_agent_exit_message_handling(test_config):
            """Test that agent exit messages are logged correctly."""
            app = VedaApp(config=test_config)
            async with app.run_test() as pilot:
                # First, create an agent tab by sending some output
                agent_role = "architect"
                pilot.app.post_message(AgentOutputMessage(role=agent_role, line="Architect planning..."))
                await pilot.pause(0.1)

                # Now, simulate the agent exiting
                exit_code = 0
                exit_message = AgentExitedMessage(role=agent_role, return_code=exit_code)
                pilot.app.post_message(exit_message)
                await pilot.pause(0.1)

                # Check main log
                main_log = pilot.app.query_one("#main-log", RichLog)
                assert f"Agent '{agent_role}' exited with code {exit_code}" in main_log.export_text(strip_styles=True)

                # Check agent log
                agent_tab_pane = pilot.app.query_one(f"#tab-{agent_role}", TabPane)
                agent_log = agent_tab_pane.query_one(RichLog)
                assert f"--- Agent '{agent_role}' exited with code {exit_code} ---" in agent_log.export_text(strip_styles=True)
