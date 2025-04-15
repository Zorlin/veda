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
    # post_message is NOT async in Textual App
    app.post_message = MagicMock()
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

        yield manager # Use yield to allow cleanup

        # Cleanup: Ensure agents are stopped after test finishes
        # Use run_sync to run the async stop function from the sync fixture finalizer
        async def stop():
             await manager.stop_all_agents()
        asyncio.run(stop())

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
@patch('agent_manager.asyncio.create_task')  # Mock create_task to avoid actual task creation
@patch('agent_manager.os.write')  # Mock os.write for initial prompt
async def test_spawn_aider_agent(mock_os_write, mock_create_task, mock_exec, mock_os_close, mock_fcntl, mock_openpty, agent_manager, mock_app):
    """Test spawning an agent that should use Aider."""
    # Configure mocks
    mock_process = AsyncMock(spec=asyncio.subprocess.Process)
    mock_process.pid = 1234
    mock_process.wait = AsyncMock(return_value=0) # Simulate process exiting cleanly later
    mock_exec.return_value = mock_process
    
    # Make create_task return a mock task
    mock_task = AsyncMock(spec=asyncio.Task)
    mock_create_task.return_value = mock_task
    
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
    # Get the expected model based on the fallback chain
    expected_model = (
        agent_manager.config.get("planner_model") or 
        agent_manager.config.get("coordinator_model") or 
        agent_manager.config.get("ollama_model")
    )
    
    agent_manager.spawn_agent.assert_called_once_with(
        role="planner",
        model=expected_model,
        initial_prompt=project_goal
    )

@pytest.mark.asyncio
@patch('agent_manager.os.write')
async def test_send_to_aider_agent(mock_os_write, agent_manager):
    """Test sending input to a running Aider agent."""
    # Spawn a mock Aider agent first
    test_role = "coder"
    agent_manager.agents[test_role] = AgentInstance(
        role=test_role,
        agent_type="aider",
        process=AsyncMock(spec=asyncio.subprocess.Process),
        master_fd=5, # Mock file descriptor
        read_task=AsyncMock(spec=asyncio.Task)
    )

    input_data = "Implement this function"
    await agent_manager.send_to_agent(test_role, input_data)

    # Check that os.write was called on the correct fd with encoded data + newline
    expected_data = (input_data + '\n').encode('utf-8')
    mock_os_write.assert_called_once_with(5, expected_data)

@pytest.mark.asyncio
async def test_send_to_ollama_agent(agent_manager, mock_app):
    """Test sending input to a running Ollama agent."""
    test_role = "planner"
    # Spawn a mock Ollama agent
    mock_ollama_client = MagicMock(spec=OllamaClient)
    agent_manager.agents[test_role] = AgentInstance(
        role=test_role,
        agent_type="ollama",
        ollama_client=mock_ollama_client
    )

    input_data = "What is the next step?"
    await agent_manager.send_to_agent(test_role, input_data)

    # Check that the worker was called via the app mock
    # The mock_run_worker executes the worker function directly.
    # We can check if the ollama_client.generate was called within the worker.
    mock_ollama_client.generate.assert_called_once_with(input_data)

    # Check that the "thinking" message was posted
    mock_app.post_message.assert_any_call(LogMessage(f"[italic grey50]Agent '{test_role}' is thinking...[/]"))
    # Check that the response message was posted (by the worker via the mock app)
    mock_app.post_message.assert_any_call(AgentOutputMessage(role=test_role, line="Mock Ollama Response"))


@pytest.mark.asyncio
@patch('agent_manager.os.close')
@patch('agent_manager.asyncio.wait_for', new_callable=AsyncMock)
async def test_stop_all_agents(mock_wait_for, mock_os_close, agent_manager):
    """Test stopping both Aider and Ollama agents."""
    # Setup mock Aider agent
    aider_role = "coder"
    mock_aider_process = AsyncMock(spec=asyncio.subprocess.Process)
    mock_aider_process.pid = 1111
    mock_aider_process.returncode = None # Indicate it's running
    mock_aider_process.terminate = MagicMock()
    mock_aider_process.kill = MagicMock()
    mock_aider_read_task = AsyncMock(spec=asyncio.Task)
    mock_aider_read_task.cancel = MagicMock()
    agent_manager.agents[aider_role] = AgentInstance(
        role=aider_role, agent_type="aider", process=mock_aider_process,
        master_fd=6, read_task=mock_aider_read_task
    )

    # Setup mock Ollama agent
    ollama_role = "planner"
    mock_ollama_client = MagicMock(spec=OllamaClient)
    agent_manager.agents[ollama_role] = AgentInstance(
        role=ollama_role, agent_type="ollama", ollama_client=mock_ollama_client
    )

    assert len(agent_manager.agents) == 2

    await agent_manager.stop_all_agents()

    # Assertions for Aider agent
    mock_aider_process.terminate.assert_called_once()
    # Check that wait_for was called, but don't compare coroutine objects directly
    mock_wait_for.assert_called_once()
    assert mock_wait_for.call_args[1]['timeout'] == 5.0 # Check timeout kwarg
    # We can't easily assert the exact coroutine object passed without more complex mocking
    mock_aider_process.kill.assert_not_called() # Assuming it terminates gracefully
    mock_aider_read_task.cancel.assert_called_once()
    mock_os_close.assert_called_with(6) # Check master_fd close

    # Assertions for Ollama agent (no process actions)
    # Check logs if needed, but main check is no process calls

    # Check agents dictionary is cleared
    assert len(agent_manager.agents) == 0

@pytest.mark.asyncio
async def test_spawn_agent_missing_model_config(mock_app, base_config, temp_work_dir):
    """Test spawning agents when model config is missing."""
    # Test missing aider_model
    config_no_aider = base_config.copy()
    original_aider_model = config_no_aider.pop("aider_model", None)
    manager_no_aider = AgentManager(app=mock_app, config=config_no_aider, work_dir=temp_work_dir)
    test_role_aider = "coder" # Uses aider
    await manager_no_aider.spawn_agent(role=test_role_aider)
    mock_app.post_message.assert_any_call(
        LogMessage(f"[bold red]Error: No aider_model configured for agent '{test_role_aider}'.[/]")
    )
    assert test_role_aider not in manager_no_aider.agents
    # No need to restore original_aider_model as we used a copy

    mock_app.post_message.reset_mock()

    # Test missing ollama_model (for a role that falls back)
    config_no_ollama = base_config.copy()
    original_ollama_model = config_no_ollama.pop("ollama_model", None)
    test_role_ollama = "skeptic" # Falls back to ollama_model
    if f"{test_role_ollama}_model" in config_no_ollama: del config_no_ollama[f"{test_role_ollama}_model"]
    # Need to patch OllamaClient for this specific instance
    with patch('agent_manager.OllamaClient', autospec=True):
        manager_no_ollama = AgentManager(app=mock_app, config=config_no_ollama, work_dir=temp_work_dir)
        await manager_no_ollama.spawn_agent(role=test_role_ollama)
        mock_app.post_message.assert_any_call(
            LogMessage(f"[bold red]Error: No model configured for Ollama agent '{test_role_ollama}'.[/]")
        )
        assert test_role_ollama not in manager_no_ollama.agents
    # No need to restore original_ollama_model

@pytest.mark.asyncio
async def test_code_reviewer_role_config(mock_app, base_config, temp_work_dir):
    """Test if code_reviewer is added to ollama_roles based on config."""
    # Case 1: Disabled
    config1 = base_config.copy()
    config1["enable_code_review"] = False
    manager1 = AgentManager(app=mock_app, config=config1, work_dir=temp_work_dir)
    assert "code_reviewer" not in manager1.ollama_roles

    # Case 2: Enabled but no specific model (should NOT be ollama role)
    config2 = base_config.copy()
    config2["enable_code_review"] = True
    config2["code_review_model"] = None
    manager2 = AgentManager(app=mock_app, config=config2, work_dir=temp_work_dir)
    assert "code_reviewer" not in manager2.ollama_roles # Assumes it would use aider if enabled without model

    # Case 3: Enabled WITH specific model (SHOULD be ollama role)
    config3 = base_config.copy()
    config3["enable_code_review"] = True
    config3["code_review_model"] = "test-reviewer-ollama"
    manager3 = AgentManager(app=mock_app, config=config3, work_dir=temp_work_dir)
    assert "code_reviewer" in manager3.ollama_roles

@pytest.mark.asyncio
async def test_code_reviewer_role_fallback(mock_app, base_config, temp_work_dir):
    """Test code_reviewer uses ollama_model when its specific model is null/missing."""
    config = base_config.copy()
    config["enable_code_review"] = True
    config["code_review_model"] = None # Explicitly null or missing

    with patch('agent_manager.OllamaClient', autospec=True) as MockOllamaClient:
        manager = AgentManager(app=mock_app, config=config, work_dir=temp_work_dir)
        manager.MockOllamaClient = MockOllamaClient # Attach mock for assertion

        # Check if code_reviewer is correctly identified as an Ollama role even with null model
        assert "code_reviewer" in manager.ollama_roles

        # Spawn the code_reviewer agent
        await manager.spawn_agent(role="code_reviewer")

        # Assert it used the fallback ollama_model
        assert "code_reviewer" in manager.agents
        agent_instance = manager.agents["code_reviewer"]
        assert agent_instance.agent_type == "ollama"
        MockOllamaClient.assert_called_once_with(
            api_url=config["ollama_api_url"],
            model=config["ollama_model"], # Check fallback model used
            timeout=config.get("ollama_request_timeout", 300),
            options=config.get("ollama_options")
        )


@pytest.mark.asyncio
@patch('agent_manager.os.close')
@patch('agent_manager.asyncio.wait_for', side_effect=asyncio.TimeoutError) # Simulate timeout
async def test_stop_all_agents_kill(mock_wait_for, mock_os_close, agent_manager):
    """Test stop_all_agents uses kill when terminate times out."""
    # Setup mock Aider agent
    aider_role = "coder"
    mock_aider_process = AsyncMock(spec=asyncio.subprocess.Process)
    mock_aider_process.pid = 2222
    mock_aider_process.returncode = None # Indicate it's running
    mock_aider_process.terminate = MagicMock()
    mock_aider_process.kill = MagicMock() # This should be called now
    mock_aider_read_task = AsyncMock(spec=asyncio.Task)
    mock_aider_read_task.cancel = MagicMock()
    agent_manager.agents[aider_role] = AgentInstance(
        role=aider_role, agent_type="aider", process=mock_aider_process,
        master_fd=7, read_task=mock_aider_read_task
    )

    await agent_manager.stop_all_agents()

    # Assertions for Aider agent
    mock_aider_process.terminate.assert_called_once()
    mock_wait_for.assert_called_once() # wait_for was called
    mock_aider_process.kill.assert_called_once() # Kill should be called after timeout
    mock_aider_read_task.cancel.assert_called_once()
    mock_os_close.assert_called_with(7) # Check master_fd close

    assert len(agent_manager.agents) == 0 # Agent should still be removed

@pytest.mark.asyncio
async def test_ollama_worker_exception(agent_manager, mock_app):
    """Test error handling when the Ollama client call fails in the worker."""
    test_role = "planner"
    mock_ollama_client = MagicMock(spec=OllamaClient)
    # Configure the mock generate method to raise an exception
    mock_exception = ValueError("Ollama API Error")
    mock_ollama_client.generate = MagicMock(side_effect=mock_exception)

    agent_manager.agents[test_role] = AgentInstance(
        role=test_role,
        agent_type="ollama",
        ollama_client=mock_ollama_client
    )

    input_data = "This will fail"
    # Directly call the worker function via the mocked run_worker
    # Note: The simplified mock_run_worker executes this synchronously
    agent_manager.app.run_worker(
        agent_manager._call_ollama_agent(agent_manager.agents[test_role], input_data),
        exclusive=True
    )

    # Check that generate was called
    mock_ollama_client.generate.assert_called_once_with(input_data)

    # Check that an error message was posted back to the app
    error_message_found = False
    for call_args in mock_app.post_message.call_args_list:
        message = call_args[0][0] # Get the first positional argument (the message)
        if isinstance(message, AgentOutputMessage) and message.role == test_role:
            if "[bold red]Error:" in message.line and "Ollama API Error" in message.line:
                error_message_found = True
                break
    assert error_message_found, "Error message from Ollama worker not found in app messages"


# TODO: Add tests for _read_pty_output (might require more complex mocking)
# TODO: Add tests for _monitor_agent_exit
