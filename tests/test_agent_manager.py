import asyncio
import pytest
from unittest.mock import MagicMock, AsyncMock, patch, call
from pathlib import Path
import sys
import os # Added for fcntl constants
import fcntl # Added for fcntl constants

# Ensure src directory is in path for imports
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
sys.path.insert(0, str(src_path))

# Now import from src
from agent_manager import AgentManager, AgentInstance, AgentOutputMessage, AgentExitedMessage, LogMessage
from ollama_client import OllamaClient # Assuming OllamaClient can be imported

# --- Fixtures ---

@pytest.fixture
def mock_app():
    """Provides a mock Textual App instance."""
    app = MagicMock()
    app.post_message = AsyncMock()
    # Mock run_worker to execute the target function directly for simplicity in some tests
    # More complex tests might need a more sophisticated mock
    async def mock_run_worker(target, *args, **kwargs):
        # If target is a coroutine function, await it
        # Note: This simplified mock executes the worker synchronously in the test thread
        # which might not perfectly replicate real-world async behavior but is useful
        # for testing the worker logic itself.
        if asyncio.iscoroutinefunction(target):
             print(f"Warning: Mock run_worker executing coroutine {target.__name__} synchronously.")
             asyncio.run(target(*args)) # Run coro synchronously for test simplicity
        # If target is a regular function (like our static worker)
        else:
            target(*args) # Execute synchronously for testing worker logic
    app.run_worker = mock_run_worker
    return app

@pytest.fixture
def base_config():
    """Provides a base configuration dictionary."""
    return {
        "ollama_model": "test-ollama-base",
        "ollama_api_url": "http://mock-ollama:11434/api/generate",
        "aider_command": "aider",
        "aider_model": "test-aider-model",
        "aider_test_command": "pytest -v",
        "project_dir": ".",
        "planner_model": "test-planner-ollama", # Example role-specific model
        "theorist_model": "test-theorist-ollama",
        "ollama_request_timeout": 10,
        "ollama_options": {},
        # Add other roles if needed for tests
    }

@pytest.fixture
def temp_work_dir(tmp_path):
    """Creates a temporary working directory."""
    work_dir = tmp_path / "workdir"
    work_dir.mkdir()
    return work_dir

@pytest.fixture
def agent_manager(mock_app, base_config, temp_work_dir):
    """Provides an AgentManager instance with mocks."""
    # Patch OllamaClient before AgentManager instantiation if needed
    with patch('agent_manager.OllamaClient', autospec=True) as MockOllamaClient:
        # Configure the mock client instance if necessary
        mock_client_instance = MockOllamaClient.return_value
        mock_client_instance.generate = MagicMock(return_value="Mock Ollama Response")

        manager = AgentManager(app=mock_app, config=base_config, work_dir=temp_work_dir)
        # Store the mock class for later assertions if needed
        manager.MockOllamaClient = MockOllamaClient
        yield manager # Use yield to allow cleanup if needed later

# --- Test Cases ---

@pytest.mark.asyncio
async def test_agent_manager_initialization(agent_manager, temp_work_dir):
    """Test if AgentManager initializes correctly."""
    assert agent_manager.work_dir == temp_work_dir
    assert "planner" in agent_manager.ollama_roles # Check a default ollama role
    assert agent_manager.app is not None
    assert agent_manager.config is not None

@pytest.mark.asyncio
@patch('agent_manager.pty.openpty', return_value=(3, 4)) # Mock pty fd creation
@patch('agent_manager.fcntl.fcntl') # Mock fcntl calls
@patch('agent_manager.os.close') # Mock os.close
@patch('agent_manager.asyncio.create_subprocess_exec', new_callable=AsyncMock)
async def test_spawn_aider_agent(mock_exec, mock_os_close, mock_fcntl, mock_openpty, agent_manager, mock_app):
    """Test spawning an agent that should use Aider."""
    mock_process = AsyncMock(spec=asyncio.subprocess.Process)
    mock_process.pid = 1234
    mock_process.wait = AsyncMock(return_value=0) # Simulate process exiting cleanly later
    mock_exec.return_value = mock_process

    # Role 'coder' is not in ollama_roles, should default to aider
    test_role = "coder"
    await agent_manager.spawn_agent(role=test_role, initial_prompt="Write hello world")

    # Assertions
    assert test_role in agent_manager.agents
    agent_instance = agent_manager.agents[test_role]
    assert agent_instance.agent_type == "aider"
    assert agent_instance.process == mock_process
    assert agent_instance.master_fd == 3 # From mock_openpty
    assert agent_instance.read_task is not None
    assert agent_instance.ollama_client is None

    # Check if subprocess was called with correct args
    mock_exec.assert_called_once()
    call_args_list = mock_exec.call_args[0] # All positional arguments
    command_parts = call_args_list[0] # First positional argument is the command tuple
    assert command_parts[0] == "aider"
    assert "--model" in command_parts
    assert agent_manager.config["aider_model"] in command_parts
    assert "--test-cmd" in command_parts
    assert agent_manager.config["aider_test_command"] in command_parts
    assert "--no-show-model-warnings" in command_parts

    # Check if pty setup was called
    mock_openpty.assert_called_once()
    mock_fcntl.assert_called_once_with(3, fcntl.F_SETFL, os.O_NONBLOCK)
    mock_os_close.assert_called_once_with(4) # Slave FD should be closed

    # Check if monitor task was created (indirectly via checking agent instance)
    # Check if reader task was created
    assert isinstance(agent_instance.read_task, asyncio.Task)

    # Check initial prompt sending (mocked send_to_agent)
    # Need to patch send_to_agent or check os.write mock if not patching send_to_agent
    # For now, assume spawn_agent calls send_to_agent correctly after delay

@pytest.mark.asyncio
async def test_spawn_ollama_agent(agent_manager, mock_app):
    """Test spawning an agent that should use Ollama."""
    test_role = "planner" # This role is in ollama_roles
    expected_model = agent_manager.config["planner_model"]

    await agent_manager.spawn_agent(role=test_role, initial_prompt="Plan the project")

    # Assertions
    assert test_role in agent_manager.agents
    agent_instance = agent_manager.agents[test_role]
    assert agent_instance.agent_type == "ollama"
    assert agent_instance.ollama_client is not None
    # Check if the correct mock OllamaClient was instantiated
    agent_manager.MockOllamaClient.assert_called_once_with(
        api_url=agent_manager.config["ollama_api_url"],
        model=expected_model,
        timeout=agent_manager.config.get("ollama_request_timeout", 300),
        options=agent_manager.config.get("ollama_options")
    )
    assert agent_instance.process is None
    assert agent_instance.master_fd is None
    assert agent_instance.read_task is None

    # Check if initial prompt triggered a worker call (via app mock)
    # The mock_run_worker executes the worker directly in this setup
    # We need to check if the worker function was called via the mock
    # This requires mocking the static worker method itself or inspecting calls differently
    # Let's check if the LogMessage for 'thinking' was posted, implying run_worker was called
    # Note: The actual worker call happens inside an asyncio.create_task,
    # so checking the direct result of spawn_agent might not be enough.
    # We rely on the side effect (post_message) for this test.
    await asyncio.sleep(0.01) # Allow task to potentially run
    mock_app.post_message.assert_any_call(LogMessage(f"[italic grey50]Agent '{test_role}' is thinking...[/]"))


@pytest.mark.asyncio
async def test_spawn_ollama_agent_fallback_model(mock_app, base_config, temp_work_dir):
    """Test Ollama agent uses fallback model if role-specific is missing."""
    test_role = "skeptic" # Assume skeptic_model is NOT in base_config initially
    if f"{test_role}_model" in base_config:
        del base_config[f"{test_role}_model"] # Ensure role-specific model is missing

    expected_model = base_config["ollama_model"] # Should fallback to this

    # Need to re-patch OllamaClient for this specific instance/test
    with patch('agent_manager.OllamaClient', autospec=True) as MockOllamaClient:
        manager = AgentManager(app=mock_app, config=base_config, work_dir=temp_work_dir)
        manager.MockOllamaClient = MockOllamaClient # Attach mock for assertion
        await manager.spawn_agent(role=test_role)

        assert test_role in manager.agents
        agent_instance = manager.agents[test_role]
        assert agent_instance.agent_type == "ollama"
        MockOllamaClient.assert_called_once_with(
            api_url=base_config["ollama_api_url"],
            model=expected_model, # Check fallback model used
            timeout=base_config.get("ollama_request_timeout", 300),
            options=base_config.get("ollama_options")
        )

@pytest.mark.asyncio
async def test_spawn_agent_already_running(agent_manager, mock_app):
    """Test attempting to spawn an agent that is already running."""
    test_role = "planner"
    # Spawn it once
    await agent_manager.spawn_agent(role=test_role)
    mock_app.post_message.reset_mock() # Reset mock calls

    # Try to spawn again
    await agent_manager.spawn_agent(role=test_role)

    # Assertions
    # Check that a warning message was posted
    mock_app.post_message.assert_called_once_with(
        LogMessage(f"[orange3]Agent '{test_role}' is already running.[/]")
    )
    # Check that the agent count didn't increase (still 1)
    assert len(agent_manager.agents) == 1

@pytest.mark.asyncio
async def test_initialize_project(agent_manager, temp_work_dir, mock_app):
    """Test the initialize_project method."""
    project_goal = "Create a test project"
    goal_file = temp_work_dir / "initial_goal.txt"

    # Mock spawn_agent to check if it's called correctly
    agent_manager.spawn_agent = AsyncMock()

    await agent_manager.initialize_project(project_goal)

    # Check if goal file was written
    assert goal_file.exists()
    assert goal_file.read_text() == project_goal

    # Check if status messages were posted
    mock_app.post_message.assert_any_call(LogMessage(f"[green]Received project goal: '{project_goal}'[/]"))
    mock_app.post_message.assert_any_call(LogMessage(f"Initial goal saved to {goal_file.name}"))

    # Check if the initial agent (planner) was spawned
    agent_manager.spawn_agent.assert_called_once_with(
        role="planner",
        model=agent_manager.config.get("coordinator_model"), # Uses coordinator model for planner
        initial_prompt=project_goal
    )

# TODO: Add tests for send_to_agent (mocking os.write and worker call)
# TODO: Add tests for stop_all_agents
# TODO: Add tests for _read_pty_output (might require more complex mocking)
# TODO: Add tests for _monitor_agent_exit
