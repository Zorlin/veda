import pytest
import asyncio
import json
import websockets
import anyio # Import the anyio library
import anyio # Import the anyio library
from unittest.mock import patch, MagicMock

from src.ui_server import UIServer

# Force only asyncio backend for tests in this file
@pytest.fixture(params=["asyncio"])
def anyio_backend(request):
    return request.param

@pytest.fixture
async def test_server(anyio_backend): # Add anyio_backend fixture back
    """Fixture to start and stop the UIServer within the test's anyio event loop."""

    # Create a memory stream pair for testing
    send_stream, receive_stream = anyio.create_memory_object_stream(float('inf'))

    # Use a different port for testing and pass the stream to the constructor
    server = UIServer(host="127.0.0.1", port=8766, receive_stream=receive_stream)
    # server.set_receive_stream(receive_stream) # Removed this line
    
    async with anyio.create_task_group() as tg:
        # Start the server in the background using the test's task group
        # Pass task_status to properly signal startup completion
        await tg.start(server.start) 
        
        # Give the server a moment to initialize fully (e.g., bind the port)
        # A more robust approach might involve waiting for a specific log message or state.
        await anyio.sleep(0.2) 
        
        # Yield the server instance to the test
        yield server
        
        # Cleanup: Cancel the task group first, then stop the server
        tg.cancel_scope.cancel()
        server.stop()
        await send_stream.aclose()
        # Add a small delay to allow the OS to release the socket
        await anyio.sleep(0.1) 

# Removed pytestmark skip logic

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_connection(test_server, anyio_backend): # Add anyio_backend fixture back
    """Test that a client can connect to the server."""
    uri = f"ws://{test_server.host}:{test_server.port}"
    
    async with websockets.connect(uri) as websocket:
        # Connection success is verified by reaching this point without error.
        # Check if initial status is received (using asyncio timeout)
        initial_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
            
        initial_status = json.loads(initial_status_str)
        assert "status" in initial_status
        assert initial_status["status"] == "Initializing"

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_broadcast(test_server, anyio_backend): # Add anyio_backend fixture back
    """Test that the server broadcasts messages to connected clients."""
    uri = f"ws://{test_server.host}:{test_server.port}"
    
    # Connect two clients
    async with websockets.connect(uri) as ws1, websockets.connect(uri) as ws2:
        # Receive initial status for both (using asyncio timeout)
        await asyncio.wait_for(ws1.recv(), timeout=1.0)
        await asyncio.wait_for(ws2.recv(), timeout=1.0)
        
        # Broadcast an update - await directly since we're in the same loop
        update_data = {"status": "Testing Broadcast", "iteration": 5, "log_entry": "Test log"}
        await test_server.broadcast(update_data)
        # Increase sleep slightly to allow broadcast processing time
        await anyio.sleep(0.1) 

        # Check if both clients received the update (using asyncio timeout)
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
async def test_ui_server_latest_status_on_connect(test_server, anyio_backend): # Add anyio_backend fixture back
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

    # Connect a new client
    async with websockets.connect(uri) as websocket:
        # Check if the received status matches the latest update (using asyncio timeout)
        latest_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
            
        latest_status = json.loads(latest_status_str)
    
    assert latest_status["status"] == "Pre-Connection Update"
    assert latest_status["run_id"] == 123
    assert "Status before connect" in latest_status["log"]

@pytest.mark.anyio
async def test_ui_server_receives_interrupt_command(test_server, anyio_backend):
    """Test that the server handles the 'interrupt' command from a client."""
    uri = f"ws://{test_server.host}:{test_server.port}"

    # Mock the harness instance and its request_interrupt method
    mock_harness = MagicMock()
    test_server.set_harness_instance(mock_harness)

    interrupt_msg = "Stop and refactor the database module."
    interrupt_flag = True

    async with websockets.connect(uri) as websocket:
        # Receive initial status
        await asyncio.wait_for(websocket.recv(), timeout=1.0)

        # Send interrupt command from client
        command = {
            "command": "interrupt",
            "message": interrupt_msg,
            "interrupt_now": interrupt_flag
        }
        await websocket.send(json.dumps(command))

        # Check for acknowledgment from server
        ack_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
        ack = json.loads(ack_str)

        assert ack["type"] == "interrupt_ack"
        assert ack["interrupt_now"] == interrupt_flag

    # Assert that the harness method was called correctly
    mock_harness.request_interrupt.assert_called_once_with(
        interrupt_msg, interrupt_now=interrupt_flag
    )


# Note: Testing the thread startup in main.py is more complex and might require
# mocking threading.Thread or using integration tests. These tests focus on the
# UIServer class functionality itself.
