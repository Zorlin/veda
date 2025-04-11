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

    # Use port 0 to let the OS assign an available ephemeral port.
    server = UIServer(host="127.0.0.1", port=0, receive_stream=receive_stream)

    async with anyio.create_task_group() as tg:
        # Start the server in the background using the test's task group
        # Pass task_status to properly signal startup completion
        await tg.start(server.start) 
        
        # Give the server a moment to initialize fully (e.g., bind the port)
        # A more robust approach might involve waiting for a specific log message or state.
        await anyio.sleep(0.2)

        # Yield both the server instance and the send stream to the test
        yield server, send_stream

        # Cleanup: Stop the server first, then cancel the fixture's task group
        server.stop() # Signal the server's internal tasks to stop
        await anyio.sleep(0.1) # Give internal tasks a moment to start cancelling
        tg.cancel_scope.cancel() # Cancel the fixture task group (which holds server.start)
        await send_stream.aclose()
        # Add delay for OS port release
        await anyio.sleep(0.5)

# Removed pytestmark skip logic

@pytest.mark.anyio # Use the correct marker for the anyio plugin
async def test_ui_server_connection(test_server, anyio_backend): # Add anyio_backend fixture back
    """Test that a client can connect to the server."""
    server, _ = test_server # Unpack server, ignore send_stream
    uri = f"ws://{server.host}:{server.port}"

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
    server, _ = test_server # Unpack server, ignore send_stream
    uri = f"ws://{server.host}:{server.port}"

    # Connect two clients
    async with websockets.connect(uri) as ws1, websockets.connect(uri) as ws2:
        # Receive initial status for both (using asyncio timeout)
        await asyncio.wait_for(ws1.recv(), timeout=1.0)
        await asyncio.wait_for(ws2.recv(), timeout=1.0)
        # Broadcast an update - await directly since we're in the same loop
        update_data = {"status": "Testing Broadcast", "iteration": 5, "log_entry": "Test log"}
        await server.broadcast(update_data) # Use unpacked server variable
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
    server, _ = test_server # Unpack server, ignore send_stream
    uri = f"ws://{server.host}:{server.port}"

    # Directly set the latest status on the server instance before connecting
    server.latest_status = { # Use unpacked server variable
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
    server, _ = test_server # Unpack server, ignore send_stream
    uri = f"ws://{server.host}:{server.port}"

    # Mock the harness instance and its request_interrupt method
    mock_harness = MagicMock()
    server.set_harness_instance(mock_harness) # Use unpacked server variable

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

@pytest.mark.anyio
async def test_ui_server_relays_stream_updates(test_server, anyio_backend):
    """Test that the server receives updates via its stream and broadcasts them."""
    # Unpack the server and send_stream from the fixture's yielded tuple
    server, send_stream = test_server
    uri = f"ws://{server.host}:{server.port}" # Use the unpacked 'server' variable

    # Get the send stream associated with the server's receive stream
    # Note: This assumes the fixture setup correctly links the streams.
    # In the fixture, we create send_stream, receive_stream and pass receive_stream to UIServer.
    # We need access to the send_stream here. Let's modify the fixture slightly.

    # Modify the fixture to yield both server and send_stream
    # (Requires changing the fixture definition) - Let's assume fixture is modified for now.
    # For this example, let's access it via the server instance if possible,
    # or ideally, the fixture should yield it.
    # Re-thinking: The fixture creates the streams. We need the send_stream from the fixture.

    # Let's modify the fixture to return both server and send_stream
    # (This requires editing the fixture code above this test)

    # Assuming the fixture `test_server` now yields `(server, send_stream)`
    server, send_stream = test_server # Unpack from modified fixture yield

    async with websockets.connect(uri) as websocket:
        # Receive initial status
        await asyncio.wait_for(websocket.recv(), timeout=1.0)

        # Simulate Harness sending an update via the stream
        test_chunk = "This is a test chunk from Aider.\n"
        update_message = {"type": "aider_output", "chunk": test_chunk}
        await send_stream.send(update_message)

        # Check if the client received the broadcast update
        received_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
        received_data = json.loads(received_str)

        assert received_data["type"] == "aider_output"
        assert received_data["chunk"] == test_chunk

        # Simulate sending a general status update
        status_update = {"status": "Processing", "iteration": 2, "log_entry": "Test log 2"}
        await send_stream.send(status_update)

        # Check if the client received the broadcast status update
        # Note: The server merges this into latest_status before broadcasting
        received_status_str = await asyncio.wait_for(websocket.recv(), timeout=1.0)
        received_status_data = json.loads(received_status_str)

        assert received_status_data["status"] == "Processing"
        assert received_status_data["iteration"] == 2
        assert "Test log 2" in received_status_data["log"]


@pytest.mark.ui # Add marker
@pytest.mark.anyio
async def test_live_log_scrollback_limit(test_server, anyio_backend):
    """Test that the server enforces the log scrollback limit."""
    server, send_stream = test_server
    uri = f"ws://{server.host}:{server.port}"
    server.max_log_lines = 3 # Set a small limit for testing

    async with websockets.connect(uri) as websocket:
        # Receive initial status
        await asyncio.wait_for(websocket.recv(), timeout=1.0)

        # Send more log entries than the limit
        for i in range(5):
            await send_stream.send({"status": f"Update {i}", "log_entry": f"Log {i}"})
            await anyio.sleep(0.05) # Allow broadcast time

        # Check the final status received by the client
        # Keep receiving until we get the last status update
        final_status_data = None
        try:
            while True:
                received_str = await asyncio.wait_for(websocket.recv(), timeout=0.5)
                final_status_data = json.loads(received_str)
                if final_status_data.get("status") == "Update 4":
                    break
        except asyncio.TimeoutError:
            # Expected if no more messages are coming
            pass

        assert final_status_data is not None, "Did not receive final status update"
        assert final_status_data["status"] == "Update 4"
        # Check that the log contains only the last 'max_log_lines' entries
        assert len(final_status_data["log"]) == server.max_log_lines
        assert final_status_data["log"] == ["Log 2", "Log 3", "Log 4"] # Should contain the last 3


@pytest.mark.ui # Add marker
@pytest.mark.anyio
async def test_live_log_prevents_duplication(test_server, anyio_backend):
    """Test that the server prevents identical consecutive log entries."""
    server, send_stream = test_server
    uri = f"ws://{server.host}:{server.port}"

    async with websockets.connect(uri) as websocket:
        # Receive initial status
        await asyncio.wait_for(websocket.recv(), timeout=1.0)

        # Send some log entries, including duplicates
        log_entries = ["Log A", "Log B", "Log B", "Log C", "Log C", "Log C", "Log D"]
        for i, entry in enumerate(log_entries):
            await send_stream.send({"status": f"Update {i}", "log_entry": entry})
            await anyio.sleep(0.05) # Allow broadcast time

        # Check the final status received by the client
        final_status_data = None
        try:
            while True:
                received_str = await asyncio.wait_for(websocket.recv(), timeout=0.5)
                final_status_data = json.loads(received_str)
                if final_status_data.get("status") == f"Update {len(log_entries) - 1}":
                    break
        except asyncio.TimeoutError:
            pass

        assert final_status_data is not None, "Did not receive final status update"
        # Check that the log contains only unique consecutive entries
        assert final_status_data["log"] == ["Log A", "Log B", "Log C", "Log D"]


# --- Placeholder/Skipped UI Tests (Require Frontend Interaction) ---

@pytest.mark.ui
@pytest.mark.skip(reason="Requires frontend rendering/interaction to verify.")
def test_diff_syntax_highlighting():
    """Check that code diffs are displayed with appropriate syntax highlighting."""
    # This needs visual inspection or a complex DOM check in a browser test.
    pass

@pytest.mark.ui
@pytest.mark.skip(reason="Requires frontend rendering/interaction to verify.")
def test_diff_viewer_prevents_duplication():
    """Ensure diff viewers don't display duplicated content chunks."""
    # Backend sends chunks; frontend JS (`processOutputBuffer`) handles assembly and prevents duplicates.
    # Testing the backend duplicate chunk prevention for the *stream* is done elsewhere.
    pass

@pytest.mark.ui
@pytest.mark.skip(reason="Requires frontend rendering/interaction to verify.")
def test_aider_control_codes_are_handled():
    """Verify Aider output correctly interprets control codes (e.g., \\c for cancel)."""
    # Backend sends raw chunks including control codes. Frontend (`ansi_up`, potentially other JS) handles interpretation.
    pass


# Note: Testing the thread startup in main.py is more complex and might require
# mocking threading.Thread or using integration tests. These tests focus on the
# UIServer class functionality itself.

# --- Fixture Modification Required for the above test ---
# The test_server fixture needs to be modified to yield the send_stream
# Add this SEARCH/REPLACE block for the fixture:
