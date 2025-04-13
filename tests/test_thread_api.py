import pytest
import requests
import subprocess
import sys
import time

def wait_for_port(port, timeout=10):
    start = time.time()
    while time.time() - start < timeout:
        try:
            import socket
            s = socket.create_connection(("localhost", port), timeout=1)
            s.close()
            return True
        except Exception:
            time.sleep(0.2)
    return False

def test_thread_api_returns_threads():
    proc = subprocess.Popen([sys.executable, "src/main.py", "start", "--prompt", "test thread api"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
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
        proc.terminate()
        proc.wait(timeout=2)
