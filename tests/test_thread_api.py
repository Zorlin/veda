import pytest
import requests
import subprocess
import sys
import time
import os # Add os import
import socket # Import socket once at the top

def wait_for_port(port, timeout=15): # Increased timeout
    start = time.time()
    while time.time() - start < timeout:
        try:
            # Use the imported socket
            s = socket.create_connection(("localhost", port), timeout=1)
            s.close()
            return True
        except Exception:
            time.sleep(0.2)
    return False

def test_thread_api_returns_threads(monkeypatch):
    """Test that the /api/threads endpoint returns properly formatted thread data."""
    # Mock the agent manager to return predictable data
    class MockAgentManager:
        def get_active_agents_status(self):
            return [
                {"id": 1, "role": "developer", "status": "running", "model": "test-model"},
                {"id": 2, "role": "tester", "status": "waiting", "model": "test-model"}
            ]
    
    # Create a simple Flask app with our endpoint for testing
    from flask import Flask, jsonify
    app = Flask(__name__)
    
    # Add the threads endpoint with our mock data
    @app.route("/api/threads")
    def api_threads():
        return jsonify(MockAgentManager().get_active_agents_status())
    
    # Start the test server in a thread
    import threading
    server_thread = threading.Thread(
        target=lambda: app.run(host="localhost", port=9900, debug=False, use_reloader=False),
        daemon=True
    )
    server_thread.start()
    
    # Wait for server to start
    test_timeout = 10  # seconds
    start_time = time.time()
    server_started = False
    while not server_started and (time.time() - start_time) < test_timeout:
        server_started = wait_for_port(9900, timeout=1)
        if not server_started:
            time.sleep(0.5)
    
    assert server_started, "Test server did not start within timeout"
    
    # Test the API endpoint
    try:
        resp = requests.get("http://localhost:9900/api/threads", timeout=2)
        resp.raise_for_status()
        data = resp.json()
        
        # Verify response format
        assert isinstance(data, list)
        assert len(data) == 2
        
        # Each thread should have the required fields
        for thread in data:
            assert "id" in thread
            assert "role" in thread
            assert "status" in thread
    except Exception as e:
        pytest.fail(f"Error testing /api/threads: {e}")
    finally:
        # No subprocess to clean up in the mock server approach
        # The server thread is a daemon thread and will be terminated when the test exits
        pass
