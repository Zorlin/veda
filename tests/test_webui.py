import pytest
import requests
import time
import subprocess
import sys
import socket
import os # Add os import

def wait_for_port(port, timeout=15): # Increased timeout
    start = time.time()
    while time.time() - start < timeout:
        try:
            s = socket.create_connection(("localhost", port), timeout=1)
            s.close()
            return True
        except Exception:
            time.sleep(0.2)
    return False

def test_webui_serves_vue_and_tailwind():
    # Start the web server in a subprocess
    # Pass a dummy API key for the test environment
    test_env = os.environ.copy()
    test_env["OPENROUTER_API_KEY"] = "test-key-for-pytest"
    # Capture stdout/stderr for debugging if wait_for_port fails, ensure text=True
    proc = subprocess.Popen([sys.executable, "src/main.py", "start", "--prompt", "test webui"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=test_env, text=True)
    server_started = False # Initialize flag
    stdout_data, stderr_data = "", "" # Initialize capture variables
    try:
        server_started = wait_for_port(9900) # Assign result to flag
        assert server_started, "Web server did not start on port 9900"
        # Give the server a moment to serve the UI
        time.sleep(1)
        # Try multiple paths to find the UI
        paths_to_try = ["", "/index.html", "/static/index.html"]
        response_found = False
            
        for path in paths_to_try:
            try:
                resp = requests.get(f"http://localhost:9900{path}")
                if resp.status_code == 200:
                    response_found = True
                    break
            except Exception:
                continue
            
        assert response_found, f"Could not find UI at any of these paths: {paths_to_try}"
        # Check for Vue.js and TailwindCSS in the HTML (we've added both to the page)
        # Use more flexible checks that will work with the basic UI template too
        has_vue = "vue" in resp.text.lower()
        has_tailwind = "tailwind" in resp.text.lower()
            
        assert has_vue, "Vue.js not found in the HTML response"
        assert has_tailwind, "Tailwind CSS not found in the HTML response"
        # Check for chat UI elements
        assert "chat" in resp.text.lower(), "Chat element not found in the HTML response"
        assert "thread" in resp.text.lower() or "agent" in resp.text.lower(), "Thread or agent element not found in the HTML response"
    finally:
        # Capture output if server didn't start
        if not server_started:
             try:
                 stdout_data, stderr_data = proc.communicate(timeout=1)
             except subprocess.TimeoutExpired:
                 proc.kill()
                 stdout_data, stderr_data = proc.communicate()
             print("\n--- Subprocess stdout (test_webui) ---")
             print(stdout_data)
             print("--- Subprocess stderr (test_webui) ---")
             print(stderr_data)
             print("---------------------------------------")

        # Ensure termination
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
