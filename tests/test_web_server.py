import pytest
import asyncio
from pathlib import Path
from unittest.mock import patch, MagicMock, AsyncMock

# Add project root to path
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
import sys
sys.path.insert(0, str(src_path))

@pytest.fixture
def mock_agent_manager():
    """Provides a mock AgentManager."""
    manager = MagicMock()
    manager.initialize_project = AsyncMock()
    manager.send_to_agent = AsyncMock()
    manager.spawn_agent = AsyncMock()
    manager.stop_all_agents = AsyncMock()
    manager.get_agent_status = MagicMock(return_value={
        "architect": "running",
        "developer": "idle"
    })
    return manager

@pytest.mark.asyncio
async def test_web_server_creation():
    """Test that the web server application is created correctly."""
    with patch('web_server.web.Application') as mock_app_class:
        from web_server import create_web_app
        
        mock_agent_manager = MagicMock()
        
        # Call the function
        app = create_web_app(mock_agent_manager)
        
        # Verify Application was created
        mock_app_class.assert_called_once()
        
        # Verify routes were added
        assert app.router.add_get.call_count > 0
        assert app.router.add_post.call_count > 0

@pytest.mark.asyncio
async def test_web_server_start():
    """Test that the web server starts correctly."""
    with patch('web_server.web.TCPSite') as mock_site:
        from web_server import start_web_server
        
        mock_app = MagicMock()
        mock_agent_manager = MagicMock()
        config = {
            "api": {
                "port": 9900,
                "host": "localhost"
            }
        }
        
        # Call the function
        await start_web_server(mock_app, mock_agent_manager, config)
        
        # Verify runner and site were created and started
        mock_site.assert_called_once()
        mock_site.return_value.start.assert_called_once()

@pytest.mark.asyncio
async def test_index_handler():
    """Test that the index handler returns the main HTML page."""
    with patch('web_server.web.FileResponse') as mock_file_response:
        from web_server import handle_index
        
        mock_request = MagicMock()
        
        # Call the handler
        await handle_index(mock_request)
        
        # Verify FileResponse was created with the index.html file
        mock_file_response.assert_called_once()
        assert "index.html" in mock_file_response.call_args[0][0]

@pytest.mark.asyncio
async def test_project_goal_handler():
    """Test that the project goal handler initializes a project with the agent manager."""
    with patch('web_server.web.json_response') as mock_json_response:
        from web_server import handle_project_goal
        
        mock_request = MagicMock()
        mock_request.json = AsyncMock(return_value={"goal": "Build a REST API"})
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.initialize_project = AsyncMock()
        
        # Call the handler
        await handle_project_goal(mock_request, mock_agent_manager)
        
        # Verify agent manager was called with the goal
        mock_agent_manager.initialize_project.assert_called_once_with("Build a REST API")
        
        # Verify response was returned
        mock_json_response.assert_called_once_with({"status": "success"})

@pytest.mark.asyncio
async def test_chat_message_handler():
    """Test that the chat message handler sends messages to the agent manager."""
    with patch('web_server.web.json_response') as mock_json_response:
        from web_server import handle_chat_message
        
        mock_request = MagicMock()
        mock_request.json = AsyncMock(return_value={"message": "Can you add authentication?", "agent": "developer"})
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.send_to_agent = AsyncMock()
        
        # Call the handler
        await handle_chat_message(mock_request, mock_agent_manager)
        
        # Verify agent manager was called with the message
        mock_agent_manager.send_to_agent.assert_called_once_with("developer", "Can you add authentication?")
        
        # Verify response was returned
        mock_json_response.assert_called_once_with({"status": "success"})

@pytest.mark.asyncio
async def test_agent_status_handler():
    """Test that the agent status handler returns the current status of all agents."""
    with patch('web_server.web.json_response') as mock_json_response:
        from web_server import handle_agent_status
        
        mock_request = MagicMock()
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.get_agent_status = MagicMock(return_value={
            "architect": "running",
            "developer": "idle"
        })
        
        # Call the handler
        await handle_agent_status(mock_request, mock_agent_manager)
        
        # Verify agent manager was called to get status
        mock_agent_manager.get_agent_status.assert_called_once()
        
        # Verify response was returned with the status
        mock_json_response.assert_called_once_with({
            "status": "success",
            "agents": {
                "architect": "running",
                "developer": "idle"
            }
        })

@pytest.mark.asyncio
async def test_spawn_agent_handler():
    """Test that the spawn agent handler creates a new agent."""
    with patch('web_server.web.json_response') as mock_json_response:
        from web_server import handle_spawn_agent
        
        mock_request = MagicMock()
        mock_request.json = AsyncMock(return_value={"role": "developer", "model": "codellama"})
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.spawn_agent = AsyncMock()
        
        # Call the handler
        await handle_spawn_agent(mock_request, mock_agent_manager)
        
        # Verify agent manager was called to spawn the agent
        mock_agent_manager.spawn_agent.assert_called_once_with("developer", "codellama", None)
        
        # Verify response was returned
        mock_json_response.assert_called_once_with({"status": "success"})

@pytest.mark.asyncio
async def test_stop_agents_handler():
    """Test that the stop agents handler stops all agents."""
    with patch('web_server.web.json_response') as mock_json_response:
        from web_server import handle_stop_agents
        
        mock_request = MagicMock()
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.stop_all_agents = AsyncMock()
        
        # Call the handler
        await handle_stop_agents(mock_request, mock_agent_manager)
        
        # Verify agent manager was called to stop all agents
        mock_agent_manager.stop_all_agents.assert_called_once()
        
        # Verify response was returned
        mock_json_response.assert_called_once_with({"status": "success"})

@pytest.mark.asyncio
async def test_websocket_handler():
    """Test that the websocket handler establishes a connection and handles messages."""
    with patch('web_server.web.WebSocketResponse') as mock_ws_response:
        from web_server import handle_websocket
        
        # Setup mock websocket
        mock_ws = MagicMock()
        mock_ws.prepare = AsyncMock()
        mock_ws.receive_json = AsyncMock(side_effect=[
            {"type": "message", "data": "Hello"},
            Exception("WebSocket closed")
        ])
        mock_ws.send_json = AsyncMock()
        mock_ws.close = AsyncMock()
        mock_ws_response.return_value = mock_ws
        
        mock_request = MagicMock()
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.send_to_agent = AsyncMock()
        
        # Call the handler
        with pytest.raises(Exception, match="WebSocket closed"):
            await handle_websocket(mock_request, mock_agent_manager)
        
        # Verify websocket was prepared
        mock_ws.prepare.assert_called_once()
        
        # Verify message was received and processed
        mock_ws.receive_json.assert_called()
        
        # Verify agent manager was called with the message
        mock_agent_manager.send_to_agent.assert_called_once()
