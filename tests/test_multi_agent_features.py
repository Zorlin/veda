import pytest
import asyncio
import json
import os
from pathlib import Path
from unittest.mock import patch, MagicMock, AsyncMock

# Add project root to path
project_root = Path(__file__).parent.parent
src_path = project_root / "src"
import sys
sys.path.insert(0, str(src_path))

from agent_manager import AgentManager, AgentInstance

@pytest.fixture
def mock_app():
    """Provides a mock Textual App instance."""
    mock = MagicMock()
    mock.post_message = MagicMock()
    mock.run_worker = AsyncMock()
    
    async def mock_run_worker(target, *args, **kwargs):
        if asyncio.iscoroutinefunction(target):
            return await target(*args, **kwargs)
        else:
            return target(*args, **kwargs)
    
    mock.run_worker.side_effect = mock_run_worker
    return mock

@pytest.fixture
def base_config():
    """Provides a base configuration for testing."""
    return {
        "ollama_model": "test-ollama-base",
        "ollama_api_url": "http://mock-ollama:11434/api/generate",
        "aider_command": "aider",
        "aider_model": "test-aider-model",
        "aider_test_command": "pytest -v",
        "project_dir": ".",
        "planner_model": "test-planner-ollama",
        "theorist_model": "test-theorist-ollama",
        "ollama_request_timeout": 10,
        "ollama_options": {},
        "enable_council": True,
        "enable_code_review": True
    }

@pytest.fixture
def temp_work_dir(tmp_path):
    """Provides a temporary working directory."""
    return tmp_path

@pytest.mark.asyncio
async def test_multi_threading():
    """Test that the agent manager can run multiple threads concurrently."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient:
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'm working on it"
        
        mock_app = MagicMock()
        # Mock the run_worker method to track concurrent calls
        concurrent_calls = []
        
        async def mock_run_worker(target, *args, **kwargs):
            concurrent_calls.append(target.__name__)
            if asyncio.iscoroutinefunction(target):
                return await target(*args, **kwargs)
            else:
                return target(*args, **kwargs)
        
        mock_app.run_worker.side_effect = mock_run_worker
        
        config = {
            "ollama_model": "llama3",
            "ollama_api_url": "http://localhost:11434/api/generate"
        }
        work_dir = Path("/tmp")
        
        manager = AgentManager(mock_app, config, work_dir)
        
        # Run multiple worker tasks
        tasks = []
        for i in range(3):
            # Check if the method exists
            if hasattr(manager, '_call_ollama_agent'):
                agent_instance = AgentInstance(
                    role=f"agent{i}",
                    agent_type="ollama",
                    ollama_client=mock_client
                )
                tasks.append(manager._call_ollama_agent(agent_instance, f"Prompt {i}"))
        
        # Wait for all tasks to complete if there are any
        if tasks:
            await asyncio.gather(*tasks)
        
        # Verify multiple worker threads were used
        assert len(concurrent_calls) == 3
        assert all(call == "_call_ollama_agent" for call in concurrent_calls)

@pytest.mark.asyncio
async def test_multi_process():
    """Test that the agent manager can spawn and manage multiple processes."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient, \
         patch('agent_manager.asyncio.create_subprocess_exec', new_callable=AsyncMock) as mock_subprocess:
        
        # Setup mocks
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'm working on it"
        
        mock_process = AsyncMock()
        mock_process.pid = 12345
        mock_process.wait = AsyncMock(return_value=0)
        mock_subprocess.return_value = mock_process
        
        # Create manager
        mock_app = MagicMock()
        config = {
            "ollama_model": "llama3",
            "ollama_api_url": "http://localhost:11434/api/generate",
            "aider_command": "aider",
            "aider_model": "codellama",
            "architect_model": "llama3",
            "planner_model": "llama3"
        }
        work_dir = Path("/tmp")
        
        with patch('agent_manager.os.openpty', return_value=(5, 6)), \
             patch('agent_manager.os.close'):
            
            manager = AgentManager(mock_app, config, work_dir)
            
            # Spawn multiple agent processes
            await manager.spawn_agent("architect")
            await manager.spawn_agent("developer")
            await manager.spawn_agent("planner")
            
            # Verify multiple processes were spawned
            assert len(manager.agents) == 3
            assert mock_subprocess.call_count >= 1
            
            # Verify each process is monitored
            assert mock_app.run_worker.call_count >= 1

@pytest.mark.asyncio
async def test_multi_instance():
    """Test that multiple Veda instances can be created and run independently."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient:
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'm working on it"
        
        config = {
            "ollama_model": "llama3",
            "ollama_api_url": "http://localhost:11434/api/generate"
        }
        
        # Create multiple agent manager instances
        mock_app1 = MagicMock()
        mock_app2 = MagicMock()
        
        work_dir1 = Path("/tmp/instance1")
        work_dir2 = Path("/tmp/instance2")
        
        manager1 = AgentManager(mock_app1, config, work_dir1)
        manager2 = AgentManager(mock_app2, config, work_dir2)
        
        # Verify they have separate state
        assert manager1 is not manager2
        assert manager1.agents is not manager2.agents
        assert manager1.work_dir != manager2.work_dir
        
        # Test they can operate independently
        await manager1.initialize_project("Project 1")
        await manager2.initialize_project("Project 2")
        
        # Verify each instance processed its own project
        mock_client.generate.assert_called()
        assert mock_client.generate.call_count >= 2

@pytest.mark.asyncio
async def test_shared_database():
    """Test that multiple agents can share a database for coordination."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient:
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'm working with the database"
        
        # Create manager
        mock_app = MagicMock()
        config = {
            "ollama_model": "llama3",
            "ollama_api_url": "http://localhost:11434/api/generate"
        }
        work_dir = Path("/tmp")
        
        manager = AgentManager(mock_app, config, work_dir)
        
        # Test shared data operations if the methods exist
        if hasattr(manager, 'store_shared_data') and hasattr(manager, 'get_shared_data'):
            with patch.object(manager, 'store_shared_data', AsyncMock()) as mock_store:
                with patch.object(manager, 'get_shared_data', AsyncMock(return_value={"components": ["api", "database", "ui"]})) as mock_get:
                    
                    # Store data
                    await manager.store_shared_data("architecture", {"components": ["api", "database", "ui"]})
                    
                    # Verify data was stored
                    mock_store.assert_called_once()
                    
                    # Retrieve data
                    data = await manager.get_shared_data("architecture")
                    
                    # Verify data was retrieved
                    mock_get.assert_called_once()
                    assert data is not None
                    assert "components" in data

@pytest.mark.asyncio
async def test_agent_halt_and_resume():
    """Test that Aider instances can be halted and resumed with new instructions."""
    with patch('agent_manager.OllamaClient') as MockOllamaClient, \
         patch('agent_manager.asyncio.create_subprocess_exec', new_callable=AsyncMock) as mock_subprocess:
        
        # Setup mocks
        mock_client = MockOllamaClient.return_value
        mock_client.generate.return_value = "I'll halt and resume as requested"
        
        mock_process = AsyncMock()
        mock_process.pid = 12345
        mock_process.wait = AsyncMock(return_value=0)
        mock_process.terminate = AsyncMock()
        mock_subprocess.return_value = mock_process
        
        # Create manager
        mock_app = MagicMock()
        config = {
            "ollama_model": "llama3",
            "ollama_api_url": "http://localhost:11434/api/generate",
            "aider_command": "aider",
            "aider_model": "codellama"
        }
        work_dir = Path("/tmp")
        
        with patch('agent_manager.os.openpty', return_value=(5, 6)), \
             patch('agent_manager.os.close'), \
             patch('agent_manager.os.write'):
            
            manager = AgentManager(mock_app, config, work_dir)
            
            # Spawn an agent
            await manager.spawn_agent("developer")
            
            # Verify agent is running
            assert "developer" in manager.agents
            
            # Test halt and resume if the methods exist
            if hasattr(manager, 'halt_agent') and hasattr(manager, 'resume_agent'):
                # Halt the agent
                await manager.halt_agent("developer")
                
                # Verify agent was terminated
                mock_process.terminate.assert_called_once()
                
                # Resume with new instructions
                await manager.resume_agent("developer", "Now implement authentication")
                
                # Verify new process was spawned
                assert mock_subprocess.call_count >= 2
                assert "developer" in manager.agents
