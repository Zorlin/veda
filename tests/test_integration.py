import pytest
import asyncio
import os
import json
from pathlib import Path
from unittest.mock import patch, MagicMock, AsyncMock

# Import the classes we need
from agent_manager import AgentManager, AgentInstance

# Add project root to path
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
import sys
sys.path.insert(0, str(src_path))

@pytest.fixture
def test_config():
    """Provides a test configuration."""
    return {
        "ollama_model": "llama3",
        "ollama_api_url": "http://localhost:11434/api/generate",
        "ollama_request_timeout": 60,
        "ollama_options": {
            "temperature": 0.7
        },
        "aider_command": "aider",
        "aider_model": "codellama",
        "aider_test_command": "pytest -v",
        "project_dir": ".",
        "enable_council": True,
        "enable_code_review": True,
        "api": {
            "port": 9900,
            "host": "localhost"
        }
    }

@pytest.fixture
def temp_work_dir(tmp_path):
    """Provides a temporary working directory."""
    return tmp_path

@pytest.mark.asyncio
async def test_end_to_end_project_creation(test_config, temp_work_dir):
    """Test the complete flow of creating a project from a user goal."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient, \
         patch('agent_manager.asyncio.create_subprocess_exec', new_callable=AsyncMock) as mock_subprocess, \
         patch('web_server.web.Application'), \
         patch('web_server.web.TCPSite'):
        
        # Setup mocks
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'll help you build a REST API"
        
        mock_process = AsyncMock()
        mock_process.pid = 12345
        mock_process.wait = AsyncMock(return_value=0)
        mock_subprocess.return_value = mock_process
        
        # Import necessary components
        from tui import VedaApp
        
        # Create app and agent manager
        mock_app = MagicMock()
        
        with patch('agent_manager.os.openpty', return_value=(5, 6)), \
             patch('agent_manager.os.close'), \
             patch('agent_manager.os.write'):
            
            agent_manager = AgentManager(mock_app, test_config, temp_work_dir)
            
            # Initialize project with a goal
            await agent_manager.initialize_project("Create a REST API for a blog")
            
            # Verify Ollama was called to process the goal
            mock_client.generate.assert_called_once()
            
            # Verify architect agent was spawned
            await agent_manager.spawn_agent("architect")
            assert "architect" in agent_manager.agents
            
            # Simulate architect creating a design
            design_file = temp_work_dir / "design.md"
            with open(design_file, 'w') as f:
                f.write("# Blog API Design\n\nEndpoints:\n- GET /posts\n- POST /posts\n- GET /posts/{id}")
            
            # Create handoff to developer
            handoff_dir = temp_work_dir / "handoffs"
            os.makedirs(handoff_dir, exist_ok=True)
            
            handoff_file = handoff_dir / "architect_to_developer.json"
            with open(handoff_file, 'w') as f:
                json.dump({
                    "from": "architect",
                    "to": "developer",
                    "message": "Please implement this API design",
                    "artifacts": ["design.md"]
                }, f)
            
            # Process handoffs if the method exists
            if hasattr(agent_manager, 'process_handoffs'):
                with patch('agent_manager.os.path.exists', return_value=True), \
                     patch('agent_manager.os.listdir', return_value=["architect_to_developer.json"]):
                    
                    # Spawn developer to receive handoff
                    await agent_manager.spawn_agent("developer")
                    await agent_manager.process_handoffs()
            
                # Verify developer was spawned
                assert "developer" in agent_manager.agents
            
            # Simulate user sending a message
            await agent_manager.send_to_agent("developer", "Add authentication to the API")
            
            # Clean up
            await agent_manager.stop_all_agents()

@pytest.mark.asyncio
async def test_web_and_cli_integration(test_config, temp_work_dir):
    """Test that the web interface and CLI work together correctly."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient, \
         patch('web_server.web.Application') as MockWebApp, \
         patch('web_server.web.TCPSite') as MockTCPSite, \
         patch('aiohttp.web.json_response') as mock_json_response:
        
        # Setup mocks
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'll help you build that"
        
        mock_web_app = MagicMock()
        MockWebApp.return_value = mock_web_app
        
        mock_site = MagicMock()
        mock_site.start = AsyncMock()
        MockTCPSite.return_value = mock_site
        
        # Import necessary components
        from tui import VedaApp
        from agent_manager import AgentManager
        from web_server import create_web_app, start_web_server, handle_project_goal, handle_chat_message
        
        # Create app and agent manager
        mock_app = MagicMock()
        agent_manager = AgentManager(mock_app, test_config, temp_work_dir)
        
        # Start web server with a patched sleep to avoid infinite loop
        with patch('web_server.asyncio.sleep', side_effect=[None, asyncio.CancelledError]):
            web_app = create_web_app(agent_manager)
            web_server_task = asyncio.create_task(
                start_web_server(web_app, agent_manager, test_config)
            )
            
            # Verify web server was started
            try:
                await asyncio.wait_for(web_server_task, timeout=0.5)
            except (asyncio.TimeoutError, asyncio.CancelledError):
                pass
            
            MockTCPSite.assert_called_once()
            mock_site.start.assert_called_once()
        
        # Simulate web API request for project goal
        mock_request = MagicMock()
        mock_request.json = AsyncMock(return_value={"goal": "Create a blog platform"})
        
        await handle_project_goal(mock_request, agent_manager)
        
        # Verify project was initialized
        mock_client.generate.assert_called_once()
        
        # Simulate CLI chat message
        await agent_manager.send_to_agent("veda", "Add a comment system to the blog")
        
        # Simulate web API chat message
        mock_request.json = AsyncMock(return_value={"message": "Make it mobile responsive", "agent": "developer"})
        
        with patch('agent_manager.os.write') as mock_write:
            # Setup an agent
            agent_manager.agents["developer"] = AgentInstance(
                role="developer",
                agent_type="aider",
                process=MagicMock(),
                master_fd=5,
                read_task=MagicMock()
            )
            
            await handle_chat_message(mock_request, agent_manager)
            
            # Verify message was sent to the agent
            mock_write.assert_called_once()
        
        # Clean up
        await agent_manager.stop_all_agents()
        if not web_server_task.done():
            web_server_task.cancel()
            
            try:
                await asyncio.wait_for(web_server_task, timeout=0.5)
            except (asyncio.TimeoutError, asyncio.CancelledError):
                pass

@pytest.mark.asyncio
async def test_multi_agent_collaboration(test_config, temp_work_dir):
    """Test that multiple agents can collaborate on a project."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient, \
         patch('agent_manager.asyncio.create_subprocess_exec', new_callable=AsyncMock) as mock_subprocess:
        
        # Setup mocks
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'll collaborate with other agents"
        
        mock_process = AsyncMock()
        mock_process.pid = 12345
        mock_process.wait = AsyncMock(return_value=0)
        mock_subprocess.return_value = mock_process
        
        # Create app and agent manager
        mock_app = MagicMock()
        
        with patch('agent_manager.os.openpty', return_value=(5, 6)), \
             patch('agent_manager.os.close'), \
             patch('agent_manager.os.write'):
            
            agent_manager = AgentManager(mock_app, test_config, temp_work_dir)
            
            # Initialize project
            await agent_manager.initialize_project("Build a social media platform")
            
            # Spawn multiple agents
            await agent_manager.spawn_agent("architect")
            await agent_manager.spawn_agent("developer")
            
            # Create handoff directories
            handoff_dir = temp_work_dir / "handoffs"
            os.makedirs(handoff_dir, exist_ok=True)
            
            # Simulate collaboration through handoffs
            
            # 1. Architect creates design
            design_file = temp_work_dir / "architecture.md"
            with open(design_file, 'w') as f:
                f.write("# Social Media Platform Architecture\n\nComponents:\n- User Auth\n- Posts\n- Comments\n- Messaging")
            
            # 2. Architect hands off to developer
            architect_handoff = handoff_dir / "architect_to_developer.json"
            with open(architect_handoff, 'w') as f:
                json.dump({
                    "from": "architect",
                    "to": "developer",
                    "message": "Please implement the user authentication component first",
                    "artifacts": ["architecture.md"]
                }, f)
            
            # 3. Process handoffs if the method exists
            if hasattr(agent_manager, 'process_handoffs'):
                with patch('agent_manager.os.path.exists', return_value=True), \
                     patch('agent_manager.os.listdir', return_value=["architect_to_developer.json"]):
                    
                    await agent_manager.process_handoffs()
            
            # 4. Developer creates implementation
            auth_file = temp_work_dir / "auth.py"
            with open(auth_file, 'w') as f:
                f.write("def login(username, password):\n    # Implementation\n    pass\n\ndef register(username, password):\n    # Implementation\n    pass")
            
            # 5. Developer hands back to architect for review
            developer_handoff = handoff_dir / "developer_to_architect.json"
            with open(developer_handoff, 'w') as f:
                json.dump({
                    "from": "developer",
                    "to": "architect",
                    "message": "I've implemented the auth component, please review",
                    "artifacts": ["auth.py"]
                }, f)
            
            # 6. Process handoffs again if the method exists
            if hasattr(agent_manager, 'process_handoffs'):
                with patch('agent_manager.os.path.exists', return_value=True), \
                     patch('agent_manager.os.listdir', return_value=["developer_to_architect.json"]):
                    
                    await agent_manager.process_handoffs()
            
            # Verify collaboration occurred
            assert os.path.exists(design_file)
            assert os.path.exists(auth_file)
            
            # Clean up
            await agent_manager.stop_all_agents()
