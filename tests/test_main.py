import subprocess
import sys
import time
import socket
import os # Added for path manipulation
import json # Added for potential future use

import pytest
from unittest.mock import MagicMock, call # Added for mocking

# Ensure src is importable
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../src")))
import chat # Import the chat module directly to mock its functions

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

def test_set_instances_manual(caplog): # Use caplog fixture
    # Clear previous logs if any
    caplog.clear()
    result = run_veda_cmd(["set", "instances", "3"])
    # Check stderr for the log message from AgentManager
    assert "Agent instances set to 3." in result.stderr
    # Optionally check stdout for the informational message from main.py
    assert "Setting options via CLI is currently informational." in result.stdout
    # Check return code is 0 for success
    assert result.returncode == 0, f"Expected return code 0, got {result.returncode}. Stderr: {result.stderr}"

def test_set_instances_auto(caplog): # Use caplog fixture
    # Clear previous logs if any
    caplog.clear()
    result = run_veda_cmd(["set", "instances", "auto"])
    # Check stderr for the log message from AgentManager
    assert "Agent instance management set to auto." in result.stderr
    # Optionally check stdout for the informational message from main.py
    assert "Setting options via CLI is currently informational." in result.stdout
    # Check return code is 0 for success
    assert result.returncode == 0, f"Expected return code 0, got {result.returncode}. Stderr: {result.stderr}"

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


# --- Tests for run_readiness_chat file reading ---

# Helper function to simulate chat interaction with file reading
def simulate_readiness_chat(monkeypatch, user_inputs, mock_responses, expected_final_prompt, tmp_path, files_to_create=None):
    """
    Simulates run_readiness_chat, mocking inputs, Ollama, and filesystem.

    Args:
        monkeypatch: Pytest monkeypatch fixture.
        user_inputs: List of strings the user will type.
        mock_responses: List of strings ollama_chat will return.
        expected_final_prompt: The expected prompt string returned by run_readiness_chat.
        tmp_path: Pytest tmp_path fixture.
        files_to_create: Dict of filename: content for files to create in tmp_path.
    """
    # Mock input
    input_iterator = iter(user_inputs)
    monkeypatch.setattr("builtins.input", lambda _: next(input_iterator))

    # Mock ollama_chat
    mock_ollama = MagicMock()
    # Ensure mock_responses has enough items, repeat last if needed
    response_iterator = iter(mock_responses + [mock_responses[-1]] * (len(user_inputs) - len(mock_responses)))
    mock_ollama.side_effect = lambda messages: next(response_iterator)
    monkeypatch.setattr(chat, "ollama_chat", mock_ollama)

    # Mock getcwd to return the tmp_path
    monkeypatch.setattr(os, "getcwd", lambda: str(tmp_path))

    # Create mock files if needed
    if files_to_create:
        for filename, content in files_to_create.items():
            file_path = tmp_path / filename
            file_path.parent.mkdir(parents=True, exist_ok=True)
            file_path.write_text(content)

    # Run the function
    final_prompt = chat.run_readiness_chat()

    # Assert the final prompt
    assert final_prompt == expected_final_prompt

    # Return the mock_ollama calls for further inspection if needed
    return mock_ollama.call_args_list


def test_readiness_chat_reads_file_successfully(monkeypatch, tmp_path):
    """Test reading an existing file during readiness chat."""
    filename = "test_readme.md"
    file_content = "This is the content of the test readme."
    files_to_create = {filename: file_content}
    user_inputs = [
        f"read {filename}",
        "yes" # Confirm readiness
    ]
    mock_responses = [
        "Okay, I see you want me to read test_readme.md. I have the content now. What should we do with it?",
        "Great! So the goal is based on the readme. Shall I start?" # Veda asks for confirmation
    ]
    expected_final_prompt = f"read {filename}" # The user message before confirmation

    calls = simulate_readiness_chat(monkeypatch, user_inputs, mock_responses, expected_final_prompt, tmp_path, files_to_create)

    # Check that ollama_chat was called with the file content context
    assert len(calls) == 2
    # The first call's messages list (index 0) is the one containing the context
    messages_sent = calls[0].args[0] # Get the 'messages' argument from the first call
    assert len(messages_sent) == 3 # system, user, user (with context)
    assert messages_sent[1]["role"] == "user"
    assert messages_sent[1]["content"] == f"read {filename}"
    assert messages_sent[2]["role"] == "user"
    assert f"Context: User asked to read '{filename}'" in messages_sent[2]["content"]
    assert file_content in messages_sent[2]["content"]


def test_readiness_chat_file_not_found(monkeypatch, tmp_path):
    """Test attempting to read a non-existent file."""
    filename = "non_existent_file.txt"
    user_inputs = [
        f"read {filename}",
        "yes" # Confirm readiness anyway
    ]
    mock_responses = [
        f"It seems '{filename}' was not found. What should we do instead?",
        "Okay, proceeding without the file. Shall I start?" # Veda asks for confirmation
    ]
    expected_final_prompt = f"read {filename}"

    calls = simulate_readiness_chat(monkeypatch, user_inputs, mock_responses, expected_final_prompt, tmp_path)

    # Check that ollama_chat was called with the system note about the missing file
    assert len(calls) == 2
    messages_sent = calls[0].args[0]
    assert len(messages_sent) == 3 # system, user, user (with system note)
    assert messages_sent[2]["role"] == "user"
    assert f"[System note: User asked to read '{filename}', but it was not found" in messages_sent[2]["content"]


def test_readiness_chat_file_outside_project(monkeypatch, tmp_path):
    """Test attempting to read a file outside the mocked project directory."""
    # Note: os.path.abspath makes testing relative paths tricky without more complex mocking.
    # This test relies on the internal check `full_path.startswith(os.getcwd())`.
    # We simulate a path that *would* resolve outside if not for the check.
    filename = "../../../etc/passwd" # A path attempting traversal
    user_inputs = [
        f"read {filename}",
        "yes" # Confirm readiness anyway
    ]
    mock_responses = [
        "I cannot access files outside the project directory.",
        "Okay, proceeding without that file. Shall I start?" # Veda asks for confirmation
    ]
    # Expected prompt is the user's request before confirmation
    expected_final_prompt = f"read {filename}"

    calls = simulate_readiness_chat(monkeypatch, user_inputs, mock_responses, expected_final_prompt, tmp_path)

    # Check that ollama_chat was called with the system note about access denial
    assert len(calls) == 2
    messages_sent = calls[0].args[0]
    assert len(messages_sent) == 3 # system, user, user (with system note)
    assert messages_sent[2]["role"] == "user"
    # The filename in the note might be normalized by abspath, so check for key phrases
    assert "[System note: Access denied trying to read" in messages_sent[2]["content"]
    assert "Inform the user." in messages_sent[2]["content"]


def test_readiness_chat_file_truncation(monkeypatch, tmp_path):
    """Test reading a file that exceeds the size limit."""
    filename = "large_file.txt"
    # Create content slightly larger than the limit (50KB)
    limit = 50 * 1024
    file_content = "A" * (limit + 100)
    files_to_create = {filename: file_content}
    user_inputs = [
        f"read {filename}",
        "yes" # Confirm readiness
    ]
    mock_responses = [
        "Okay, I have read the content of large_file.txt, although it was truncated due to its size. What's next?",
        "Okay, proceeding with the truncated file info. Shall I start?" # Veda asks for confirmation
    ]
    expected_final_prompt = f"read {filename}"

    calls = simulate_readiness_chat(monkeypatch, user_inputs, mock_responses, expected_final_prompt, tmp_path, files_to_create)

    # Check that ollama_chat was called with the truncated content and note
    assert len(calls) == 2
    messages_sent = calls[0].args[0]
    assert len(messages_sent) == 3 # system, user, user (with context)
    assert messages_sent[2]["role"] == "user"
    assert f"Context: User asked to read '{filename}'" in messages_sent[2]["content"]
    assert file_content[:limit] in messages_sent[2]["content"] # Check beginning is present
    assert "[... file truncated due to size limit ...]" in messages_sent[2]["content"] # Check truncation note
