import pytest
from pathlib import Path
import sys
from unittest.mock import patch, MagicMock, AsyncMock

# Add project root to path
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
sys.path.insert(0, str(src_path))

@pytest.fixture
def test_config():
    """Basic configuration for testing the web interface."""
    return {
        "ollama_api_url": "http://localhost:11434/api/generate",
        "ollama_model": "mock_model",
        "ollama_request_timeout": 10,
        "ollama_options": {},
        "api": {
            "port": 9900,
            "host": "localhost"
        }
    }

@pytest.mark.asyncio
async def test_web_server_starts():
    """Test that the web server starts on the configured port."""
    with patch('aiohttp.web.AppRunner') as mock_runner, \
         patch('aiohttp.web.TCPSite') as mock_site_class, \
         patch('web_server.asyncio.sleep', side_effect=asyncio.CancelledError):
        from web_server import start_web_server
        
        mock_app = MagicMock()
        mock_agent_manager = MagicMock()
        mock_site = mock_site_class.return_value
        
        config = {
            "api": {
                "port": 9900,
                "host": "localhost"
            }
        }
        
        # The test will raise CancelledError to exit the infinite loop
        with pytest.raises(asyncio.CancelledError):
            await start_web_server(mock_app, mock_agent_manager, config)
        
        # Verify the server was started with correct host/port
        mock_runner.assert_called_once_with(mock_app)
        mock_runner.return_value.setup.assert_called_once()
        mock_site_class.assert_called_once_with(mock_runner.return_value, 'localhost', 9900)
        mock_site.start.assert_called_once()

@pytest.mark.asyncio
async def test_web_api_endpoints():
    """Test that the web API endpoints are registered and respond correctly."""
    with patch('aiohttp.web.Application') as mock_app_class:
        from web_server import create_web_app
        
        mock_app = mock_app_class.return_value
        mock_agent_manager = MagicMock()
        
        create_web_app(mock_agent_manager)
        
        # Verify routes were added
        assert mock_app.router.add_get.call_count > 0
        assert mock_app.router.add_post.call_count > 0

@pytest.mark.asyncio
async def test_web_chat_endpoint():
    """Test that the chat endpoint processes messages correctly."""
    with patch('aiohttp.web.json_response') as mock_json_response:
        from web_server import handle_chat_message
        
        mock_request = MagicMock()
        mock_request.json = AsyncMock(return_value={"message": "Build a web app"})
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.send_to_agent = AsyncMock()
        
        # Call the handler
        await handle_chat_message(mock_request, mock_agent_manager)
        
        # Verify agent manager was called to process the message
        mock_agent_manager.send_to_agent.assert_called_once()
        mock_json_response.assert_called_once()

@pytest.mark.asyncio
async def test_web_project_goal_submission():
    """Test that project goals can be submitted via the web interface."""
    with patch('aiohttp.web.json_response') as mock_json_response:
        from web_server import handle_project_goal
        
        mock_request = MagicMock()
        mock_request.json = AsyncMock(return_value={"goal": "Create a REST API"})
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.initialize_project = AsyncMock()
        
        # Call the handler
        await handle_project_goal(mock_request, mock_agent_manager)
        
        # Verify project was initialized with the goal
        mock_agent_manager.initialize_project.assert_called_once_with("Create a REST API")
        mock_json_response.assert_called_once()

@pytest.mark.asyncio
async def test_web_agent_status_endpoint():
    """Test that agent status can be retrieved via the web interface."""
    with patch('aiohttp.web.json_response') as mock_json_response:
        from web_server import handle_agent_status
        
        mock_request = MagicMock()
        
        mock_agent_manager = MagicMock()
        mock_agent_manager.get_agent_status = MagicMock(return_value={
            "architect": "running",
            "developer": "idle"
        })
        
        # Call the handler
        await handle_agent_status(mock_request, mock_agent_manager)
        
        # Verify status was retrieved and returned
        mock_agent_manager.get_agent_status.assert_called_once()
        mock_json_response.assert_called_once()

@pytest.mark.asyncio
async def test_web_static_files():
    """Test that static files (HTML, CSS, JS) are served correctly."""
    with patch('aiohttp.web.Application') as mock_app_class:
        from web_server import create_web_app
        
        mock_app = mock_app_class.return_value
        mock_agent_manager = MagicMock()
        
        create_web_app(mock_agent_manager)
        
        # Verify static routes were added
        mock_app.router.add_static.assert_called_once()
