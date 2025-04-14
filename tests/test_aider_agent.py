import os
import sys
import time
import threading
import types
from collections import deque
import pytest

import sys
import os

# Ensure src/ is on sys.path for import
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../src")))

# Import the specific modules needed directly after modifying sys.path
import agent_manager as agent_manager_module # Alias the import
import constants
import subprocess # Import subprocess to patch it

class DummyProcess:
    def __init__(self, output_lines):
        self._output_lines = output_lines
        self.stdout = self
        self._index = 0
        self.returncode = None
        self._terminated = False

    def poll(self):
        return self.returncode if self._terminated else None

    def readline(self):
        if self._index < len(self._output_lines):
            line = self._output_lines[self._index]
            self._index += 1
            return line + "\n"
        else:
            # Simulate blocking read until termination
            while not self._terminated:
                time.sleep(0.01)
            return '' # Return empty string when terminated

    def terminate(self):
        self._terminated = True
        self.returncode = 0 # Simulate successful termination

    def wait(self, timeout=None):
        # Simulate waiting for termination
        start_time = time.time()
        while not self._terminated:
            if timeout is not None and (time.time() - start_time) > timeout:
                raise subprocess.TimeoutExpired(self._cmd, timeout)
            time.sleep(0.01)
        self._terminated = True
        self.returncode = 0

    def kill(self):
        self._terminated = True
        self.returncode = -9

    def close(self):
        pass

@pytest.fixture
def agent_manager(monkeypatch):
    # Patch subprocess.Popen to simulate Aider
    def fake_popen(cmd, stdout, stderr, text, bufsize, universal_newlines, env):
        # Simulate Aider's prompt/response cycle
        output_lines = [
            "Apply changes? [y/n/q/a/v]",
            "Proceed? [y/n]",
            "Aider: All done!"
        ]
        proc = DummyProcess(output_lines)
        proc._cmd = cmd # Store command for potential TimeoutExpired error
        return proc

    # Patch subprocess.Popen within the agent_manager module
    monkeypatch.setattr(agent_manager_module.subprocess, "Popen", fake_popen)
    # Patch OPENROUTER_API_KEY within the agent_manager module where it's checked
    monkeypatch.setattr(agent_manager_module, "OPENROUTER_API_KEY", "dummy-key")
    # Patch the primary model constant within the agent_manager module
    monkeypatch.setattr(agent_manager_module, "AIDER_PRIMARY_MODEL", "gemma3:12b")
    # Return a fresh AgentManager instance from the correct module
    return agent_manager_module.AgentManager()

def test_aider_agent_starts_and_handles_prompts(agent_manager: agent_manager_module.AgentManager): # Use alias in type hint
    prompt = "Test: improve the codebase"
    # Use the developer role as per the updated AgentManager logic
    role = "developer"
    agent_manager._start_aider_agent(role, prompt, model="gemma3:12b")

    # Wait significantly longer for the output thread and potential sleeps in DummyProcess
    time.sleep(0.5)

    agent_info = agent_manager.active_agents.get(role)
    assert agent_info is not None
    output = list(agent_info["output"])
    # Check that Aider prompts are present
    # Check that Aider prompts are present in the output buffer
    # Use a loop with timeout to wait for expected output, as thread timing can vary
    start_time = time.time()
    expected_outputs_found = {
        "Apply changes?": False,
        "Proceed?": False,
        "Aider: All done!": False
    }
    all_found = False
    while time.time() - start_time < 2: # Wait up to 2 seconds
        output = list(agent_info["output"])
        expected_outputs_found["Apply changes?"] = any("Apply changes?" in line for line in output)
        expected_outputs_found["Proceed?"] = any("Proceed?" in line for line in output)
        expected_outputs_found["Aider: All done!"] = any("Aider: All done!" in line for line in output)
        if all(expected_outputs_found.values()):
            all_found = True
            break
        time.sleep(0.1)

    assert all_found, f"Did not find all expected outputs. Found: {expected_outputs_found}"

    # Simulate agent finishing by terminating the dummy process
    agent_info["process"].terminate()
    # Wait for the monitor loop to potentially update status (or call directly)
    time.sleep(0.1) # Give monitor loop a chance (though calling directly is more reliable)
    agent_manager._update_agent_statuses() # Call directly for deterministic test

    # Check status after update
    final_status = agent_manager.active_agents.get(role)["status"]
    assert "finished" in final_status, f"Agent status should be 'finished', but was '{final_status}'"
