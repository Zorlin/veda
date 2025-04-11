import pytest
import asyncio
import json
import websockets
import anyio # Import the anyio library
from unittest.mock import patch, MagicMock

from src.ui_server import UIServer

@pytest.fixture
async def test_server(anyio_backend):
    """Fixture to start and stop the UIServer within the test's anyio event loop."""
    # Use a different port for testing
    server = UIServer(host="127.0.0.1", port=8766) 
    
    # Create a memory stream pair for testing
    send_stream, receive_stream = anyio.create_memory_object_stream(float('inf'))
    server.set_receive_stream(receive_stream)
    
    async with anyio.create_task_group() as tg:
        # Start the server in the background using the test's task group
        server_task = await tg.start(server.start)
        
        # Give the server a moment to initialize fully (e.g., bind the port)
        # A more robust approach might involve waiting for a specific log message or state.
        await anyio.sleep(0.2) 
        
        # Yield the server instance to the test
        yield server
        
        # Cleanup: Signal the server to stop and cancel the task group
        server.stop()
        await send_stream.aclose()
        tg.cancel_scope.cancel()

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_connection(test_server, anyio_backend):
    """Test that a client can connect to the server."""
    uri = f"ws://{test_server.host}:{test_server.port}"
    
    try:
        async with websockets.connect(uri) as websocket:
            # Connection success is verified by reaching this point without error.
            # Check if initial status is received
            if anyio_backend == "asyncio":
                initial_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
            else:  # trio
                initial_status_str = await anyio.fail_after(1.0, websocket.recv)
                
            initial_status = json.loads(initial_status_str)
            assert "status" in initial_status
            assert initial_status["status"] == "Initializing"
    except RuntimeError as e:
        if anyio_backend == "trio" and "no running event loop" in str(e):
            pytest.skip(f"Skipping due to websockets compatibility issue with trio: {e}")
        else:
            raise

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_broadcast(test_server, anyio_backend):
    """Test that the server broadcasts messages to connected clients."""
    uri = f"ws://{test_server.host}:{test_server.port}"
    
    try:
        # Connect two clients
        async with websockets.connect(uri) as ws1, websockets.connect(uri) as ws2:
            # Receive initial status for both
            if anyio_backend == "asyncio":
                await asyncio.wait_for(ws1.recv(), timeout=1.0)
                await asyncio.wait_for(ws2.recv(), timeout=1.0)
            else:  # trio
                await anyio.fail_after(1.0, ws1.recv)
                await anyio.fail_after(1.0, ws2.recv)
            
            # Broadcast an update - await directly since we're in the same loop
            update_data = {"status": "Testing Broadcast", "iteration": 5, "log_entry": "Test log"}
            await test_server.broadcast(update_data)
            # Increase sleep slightly to allow broadcast processing time
            await anyio.sleep(0.1) 

            # Check if both clients received the update
            if anyio_backend == "asyncio":
                update1_str = await asyncio.wait_for(ws1.recv(), timeout=1.0)
                update2_str = await asyncio.wait_for(ws2.recv(), timeout=1.0)
            else:  # trio
                update1_str = await anyio.fail_after(1.0, ws1.recv)
                update2_str = await anyio.fail_after(1.0, ws2.recv)
        
        update1 = json.loads(update1_str)
        update2 = json.loads(update2_str)
        
        assert update1["status"] == "Testing Broadcast"
        assert update1["iteration"] == 5
        assert "Test log" in update1["log"]
        
        assert update2["status"] == "Testing Broadcast"
        assert update2["iteration"] == 5
        assert "Test log" in update2["log"]
    except RuntimeError as e:
        if anyio_backend == "trio" and "no running event loop" in str(e):
            pytest.skip(f"Skipping due to websockets compatibility issue with trio: {e}")
        else:
            raise

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_latest_status_on_connect(test_server, anyio_backend):
    """Test that a new client receives the *latest* status upon connection."""
    uri = f"ws://{test_server.host}:{test_server.port}"

    # Directly set the latest status on the server instance before connecting
    test_server.latest_status = {
        "status": "Pre-Connection Update", 
        "run_id": 123, 
        "iteration": 1, # Make sure iteration is also set if expected
        "log": ["Status before connect"] # Ensure log is updated directly
    }
    # No need to call broadcast or sleep if we set the state directly for the test

    try:
        # Connect a new client
        async with websockets.connect(uri) as websocket:
            # Check if the received status matches the latest update
            if anyio_backend == "asyncio":
                latest_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
            else:  # trio
                latest_status_str = await anyio.fail_after(1.0, websocket.recv)
                
            latest_status = json.loads(latest_status_str)
        
        assert latest_status["status"] == "Pre-Connection Update"
        assert latest_status["run_id"] == 123
        assert "Status before connect" in latest_status["log"]
    except RuntimeError as e:
        if anyio_backend == "trio" and "no running event loop" in str(e):
            pytest.skip(f"Skipping due to websockets compatibility issue with trio: {e}")
        else:
            raise

# Note: Testing the thread startup in main.py is more complex and might require
# mocking threading.Thread or using integration tests. These tests focus on the
# UIServer class functionality itself.
