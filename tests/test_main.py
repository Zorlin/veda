import subprocess
import sys
import time
import socket

import pytest

def run_veda_cmd(args):
    """Helper to run the CLI and capture output."""
    result = subprocess.run(
        [sys.executable, "src/main.py"] + args,
        capture_output=True,
        text=True,
        timeout=10
    )
    return result

def test_help_message():
    result = run_veda_cmd([])
    assert "Veda - Software development that doesn't sleep." in result.stdout
    assert "start" in result.stdout
    assert "set" in result.stdout
    assert "chat" in result.stdout

def test_set_instances_manual():
    result = run_veda_cmd(["set", "instances", "3"])
    assert "Agent instances set to 3." in result.stderr or "Agent instances set to 3." in result.stdout

def test_set_instances_auto():
    result = run_veda_cmd(["set", "instances", "auto"])
    assert "Agent instance management set to auto." in result.stderr or "Agent instance management set to auto." in result.stdout

def test_chat_interface(monkeypatch):
    # Simulate user typing 'exit' immediately
    inputs = iter(["exit"])
    monkeypatch.setattr("builtins.input", lambda _: next(inputs))
    result = run_veda_cmd(["chat"])
    assert "Welcome to Veda chat" in result.stdout or "Welcome to Veda chat" in result.stderr

import os # Add os import

def test_web_server_starts():
    # Start the web server in a subprocess and check if port 9900 is open
    # Pass a dummy API key for the test environment
    test_env = os.environ.copy()
    test_env["OPENROUTER_API_KEY"] = "test-key-for-pytest"
    # Explicitly capture stdout/stderr and use text=True
    proc = subprocess.Popen([sys.executable, "src/main.py", "start"], env=test_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(5) # Increased wait time for server startup
    s = socket.socket()
    connected = False # Initialize before try
    stdout, stderr = "", "" # Initialize before try
    try:
        s.connect(("localhost", 9900))
        connected = True
    except Exception:
        connected = False
        # No need to initialize stdout/stderr here anymore
    finally:
        s.close()
        # Capture output before terminating if connection failed
        if not connected:
            try:
                # Ensure text=True for string output
                stdout, stderr = proc.communicate(timeout=1)
            except subprocess.TimeoutExpired:
                proc.kill()
                stdout, stderr = proc.communicate()

        # Ensure termination
        if proc.poll() is None:
             proc.terminate()
             try:
                 proc.wait(timeout=2)
             except subprocess.TimeoutExpired:
                 proc.kill()
                 proc.wait() # Wait for kill

    if not connected:
        print("\n--- Subprocess stdout (test_web_server_starts) ---")
        print(stdout if stdout else "(No stdout)")
        print("--- Subprocess stderr (test_web_server_starts) ---")
        print(stderr if stderr else "(No stderr)")
        print("-------------------------------------------------")

    assert connected, "Web server did not start on port 9900"
