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

def test_thread_api_returns_threads():
    # Pass a dummy API key for the test environment
    test_env = os.environ.copy()
    test_env["OPENROUTER_API_KEY"] = "test-key-for-pytest"
    proc = subprocess.Popen([sys.executable, "src/main.py", "start", "--prompt", "test thread api"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=test_env)
    try:
        assert wait_for_port(9900), "Web server did not start on port 9900"
        # Give the server a moment to serve the API
        time.sleep(1)
        resp = requests.get("http://localhost:9900/api/threads")
        assert resp.status_code == 200
        data = resp.json()
        assert isinstance(data, list)
        # Each thread should have at least id, role, and status
        for thread in data:
            assert "id" in thread
            assert "role" in thread
            assert "status" in thread
    finally:
        # Capture output if server didn't start
        if not server_started:
             try:
                 stdout_data, stderr_data = proc.communicate(timeout=1)
             except subprocess.TimeoutExpired:
                 proc.kill()
                 stdout_data, stderr_data = proc.communicate()
             print("\n--- Subprocess stdout (test_thread_api) ---")
             print(stdout_data)
             print("--- Subprocess stderr (test_thread_api) ---")
             print(stderr_data)
             print("------------------------------------------")

        # Ensure termination
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
