import pytest
import asyncio
import json
import websockets
from unittest.mock import patch, MagicMock

from src.ui_server import UIServer

@pytest.fixture
async def test_server():
    """Fixture to start and stop the UIServer for testing."""
    server = UIServer(host="127.0.0.1", port=8766) # Use a different port for testing
    server_task = asyncio.create_task(server.start())
    server.server_task = server_task # Link task for send_update
    # Give the server a moment to start up
    await asyncio.sleep(0.1)
    yield server
    # Cleanup: stop the server
    server.stop()
    try:
        await asyncio.wait_for(server_task, timeout=2.0)
    except asyncio.TimeoutError:
        print("Warning: Server task did not finish cleanly during test cleanup.")
        server_task.cancel() # Force cancel if needed

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_connection(test_server):
    """Test that a client can connect to the server."""
    uri = f"ws://{test_server.host}:{test_server.port}"
    async with websockets.connect(uri) as websocket:
        assert websocket.is_open() # Use the correct method to check if open
        # Check if initial status is received
        initial_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
        initial_status = json.loads(initial_status_str)
        assert "status" in initial_status
        assert initial_status["status"] == "Initializing"

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_broadcast(test_server):
    """Test that the server broadcasts messages to connected clients."""
    uri = f"ws://{test_server.host}:{test_server.port}"
    
    # Connect two clients
    async with websockets.connect(uri) as ws1, websockets.connect(uri) as ws2:
        # Receive initial status for both
        await asyncio.wait_for(ws1.recv(), timeout=1.0)
        await asyncio.wait_for(ws2.recv(), timeout=1.0)
        
        # Broadcast an update
        update_data = {"status": "Testing Broadcast", "iteration": 5, "log_entry": "Test log"}
        # Use run_coroutine_threadsafe as send_update does, simulating harness call
        asyncio.run_coroutine_threadsafe(test_server.broadcast(update_data), test_server.server_task.get_loop()).result(timeout=1)

        # Check if both clients received the update
        update1_str = await asyncio.wait_for(ws1.recv(), timeout=1.0)
        update2_str = await asyncio.wait_for(ws2.recv(), timeout=1.0)
        
        update1 = json.loads(update1_str)
        update2 = json.loads(update2_str)
        
        assert update1["status"] == "Testing Broadcast"
        assert update1["iteration"] == 5
        assert "Test log" in update1["log"]
        
        assert update2["status"] == "Testing Broadcast"
        assert update2["iteration"] == 5
        assert "Test log" in update2["log"]

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_latest_status_on_connect(test_server):
    """Test that a new client receives the *latest* status upon connection."""
    uri = f"ws://{test_server.host}:{test_server.port}"

    # Send an update before the client connects
    update_data = {"status": "Pre-Connection Update", "run_id": 123, "log_entry": "Status before connect"}
    asyncio.run_coroutine_threadsafe(test_server.broadcast(update_data), test_server.server_task.get_loop()).result(timeout=1)

    # Connect a new client
    async with websockets.connect(uri) as websocket:
        # Check if the received status matches the latest update
        latest_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
        latest_status = json.loads(latest_status_str)
        
        assert latest_status["status"] == "Pre-Connection Update"
        assert latest_status["run_id"] == 123
        assert "Status before connect" in latest_status["log"]

# Note: Testing the thread startup in main.py is more complex and might require
# mocking threading.Thread or using integration tests. These tests focus on the
# UIServer class functionality itself.
