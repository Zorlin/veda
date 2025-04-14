import os
import sys
import time
import threading
import types
from collections import deque
import pytest

import src.main as veda_main

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
            time.sleep(0.01)
            return ''
    
    def terminate(self):
        self._terminated = True
        self.returncode = 0

    def wait(self, timeout=None):
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
        return DummyProcess(output_lines)
    monkeypatch.setattr(veda_main.subprocess, "Popen", fake_popen)
    # Patch OPENROUTER_API_KEY to a dummy value
    monkeypatch.setattr(veda_main, "OPENROUTER_API_KEY", "dummy-key")
    # Patch model to gemma3:12b
    monkeypatch.setattr(veda_main, "AIDER_PRIMARY_MODEL", "gemma3:12b")
    # Return a fresh AgentManager
    return veda_main.AgentManager()

def test_aider_agent_starts_and_handles_prompts(agent_manager):
    prompt = "Test: improve the codebase"
    agent_manager._start_aider_agent("aider", prompt, model="gemma3:12b")
    # Wait for output thread to process
    time.sleep(0.1)
    agent_info = agent_manager.active_agents.get("aider")
    assert agent_info is not None
    output = list(agent_info["output"])
    # Check that Aider prompts are present
    assert any("Apply changes?" in line for line in output)
    assert any("Proceed?" in line for line in output)
    assert any("Aider: All done!" in line for line in output)
    # Simulate agent finishing
    agent_info["process"].terminate()
    agent_manager._update_agent_statuses()
    assert "finished" in agent_info["status"]
