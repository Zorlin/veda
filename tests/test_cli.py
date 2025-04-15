import pytest
import asyncio
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock, AsyncMock

# Add project root to path
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
sys.path.insert(0, str(src_path))

@pytest.fixture
def mock_agent_manager():
    """Provides a mock AgentManager."""
    manager = MagicMock()
    manager.initialize_project = AsyncMock()
    manager.send_to_agent = AsyncMock()
    manager.spawn_agent = AsyncMock()
    manager.stop_all_agents = AsyncMock()
    return manager

@pytest.mark.asyncio
async def test_cli_start_command():
    """Test that the CLI start command launches Veda correctly."""
    with patch('cli.VedaApp') as MockVedaApp, \
         patch('cli.AgentManager') as MockAgentManager, \
         patch('cli.load_config') as mock_load_config, \
         patch('cli.start_web_server') as mock_start_web_server:
        
        # Setup mocks
        mock_app = MagicMock()
        MockVedaApp.return_value = mock_app
        mock_app.run = AsyncMock()
        
        mock_agent_manager = MagicMock()
        MockAgentManager.return_value = mock_agent_manager
        
        mock_load_config.return_value = {
            "api": {
                "port": 9900,
                "host": "localhost"
            }
        }
        
        # Call the start command
        from cli import start_command
        await start_command()
        
        # Verify app and agent manager were created
        MockVedaApp.assert_called_once()
        MockAgentManager.assert_called_once()
        
        # Verify web server was started
        mock_start_web_server.assert_called_once()
        
        # Verify app was run
        mock_app.run.assert_called_once()

@pytest.mark.asyncio
async def test_cli_chat_command():
    """Test that the CLI chat command starts a chat session with Veda."""
    with patch('cli.VedaApp') as MockVedaApp, \
         patch('cli.AgentManager') as MockAgentManager, \
         patch('cli.load_config') as mock_load_config, \
         patch('builtins.input', side_effect=["Hello", "Build a web app", KeyboardInterrupt]):
        
        # Setup mocks
        mock_app = MagicMock()
        MockVedaApp.return_value = mock_app
        mock_app.run = AsyncMock()
        
        mock_agent_manager = MagicMock()
        MockAgentManager.return_value = mock_agent_manager
        mock_agent_manager.send_to_agent = AsyncMock()
        
        mock_load_config.return_value = {}
        
        # Call the chat command
        from cli import chat_command
        await chat_command()
        
        # Verify agent manager was called with the messages
        assert mock_agent_manager.send_to_agent.call_count == 2

@pytest.mark.asyncio
async def test_cli_stop_command():
    """Test that the CLI stop command stops all Veda services."""
    with patch('cli.AgentManager') as MockAgentManager, \
         patch('cli.load_config') as mock_load_config, \
         patch('cli.os.path.exists', return_value=True), \
         patch('cli.os.kill') as mock_kill:
        
        # Setup mocks
        mock_agent_manager = MagicMock()
        MockAgentManager.return_value = mock_agent_manager
        mock_agent_manager.stop_all_agents = AsyncMock()
        
        mock_load_config.return_value = {}
        
        # Mock PID file
        with patch('cli.open', create=True) as mock_open:
            mock_file = MagicMock()
            mock_file.read.return_value = "12345"
            mock_open.return_value.__enter__.return_value = mock_file
            
            # Call the stop command
            from cli import stop_command
            await stop_command()
            
            # Verify agents were stopped
            mock_agent_manager.stop_all_agents.assert_called_once()
            
            # Verify process was killed
            mock_kill.assert_called_once_with(12345, 15)  # SIGTERM

@pytest.mark.asyncio
async def test_cli_help_command():
    """Test that the CLI help command displays help information."""
    with patch('builtins.print') as mock_print:
        # Call the help command
        from cli import help_command
        await help_command()
        
        # Verify help was printed
        assert mock_print.call_count > 0
        
        # Check for key commands in the help text
        help_text = ' '.join(str(call[0][0]) for call in mock_print.call_args_list)
        assert "start" in help_text
        assert "chat" in help_text
        assert "stop" in help_text

@pytest.mark.asyncio
async def test_cli_status_command():
    """Test that the CLI status command displays Veda's status."""
    with patch('cli.os.path.exists', side_effect=[True, True]), \
         patch('cli.open', create=True) as mock_open, \
         patch('builtins.print') as mock_print:
        
        # Mock PID file
        mock_file = MagicMock()
        mock_file.read.return_value = "12345"
        mock_open.return_value.__enter__.return_value = mock_file
        
        # Mock process check
        with patch('cli.os.kill', return_value=None):
            # Call the status command
            from cli import status_command
            await status_command()
            
            # Verify status was printed
            assert mock_print.call_count > 0
            
            # Check for running status in the output
            status_text = ' '.join(str(call[0][0]) for call in mock_print.call_args_list)
            assert "running" in status_text.lower()

@pytest.mark.asyncio
async def test_cli_detach_handling():
    """Test that the CLI handles Ctrl+D (detach) correctly."""
    with patch('cli.VedaApp') as MockVedaApp, \
         patch('cli.AgentManager') as MockAgentManager, \
         patch('cli.load_config') as mock_load_config, \
         patch('builtins.input', side_effect=EOFError):
        
        # Setup mocks
        mock_app = MagicMock()
        MockVedaApp.return_value = mock_app
        mock_app.run = AsyncMock()
        
        mock_agent_manager = MagicMock()
        MockAgentManager.return_value = mock_agent_manager
        
        # Add handle_user_detach method if it exists
        if hasattr(AgentManager, 'handle_user_detach'):
            mock_agent_manager.handle_user_detach = AsyncMock()
        
        mock_load_config.return_value = {}
        
        # Call the chat command
        from cli import chat_command
        await chat_command()
        
        # Verify detach was handled if the method exists
        if hasattr(mock_agent_manager, 'handle_user_detach'):
            mock_agent_manager.handle_user_detach.assert_called_once()
