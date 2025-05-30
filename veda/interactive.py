#!/usr/bin/env python3
"""
Veda Interactive - TUI for managing multiple Claude Code instances
"""
import asyncio
import json
import os
import pty
import select
import subprocess
import sys
import termios
import tty
from pathlib import Path
from typing import List, Dict, Optional, Tuple
import aiohttp
from textual.app import App, ComposeResult
from textual.widgets import Header, Footer, TextArea, TabbedContent, TabPane, Label
from textual.containers import Horizontal, Vertical, Container
from textual.binding import Binding
from textual import events
from rich.text import Text
from rich.syntax import Syntax
import time


class OllamaDecisionMaker:
    """Uses Ollama to make decisions when Claude asks questions"""
    
    def __init__(self, base_url="http://localhost:11434"):
        self.base_url = base_url
        self.model = "deepseek-r1:14b"
        self.decision_history = []
    
    async def make_decision(self, context: str, question: str) -> str:
        """Make a decision based on context and question"""
        system_prompt = """You are an AI assistant helping coordinate multiple Claude Code instances.
        When Claude asks a question or needs a decision, analyze the context and provide a clear, concise answer.
        
        Important guidelines:
        - If asked about file changes, generally approve unless obviously destructive
        - If asked about creating files, approve if it aligns with the task
        - If asked to confirm actions, say "yes" or provide the requested input
        - Be decisive and brief - Claude is waiting for your response
        - Consider the overall task context when making decisions
        
        Respond with just the answer Claude needs, no explanation."""
        
        # Add recent history for context
        history_context = "\n".join(self.decision_history[-5:]) if self.decision_history else ""
        
        async with aiohttp.ClientSession() as session:
            payload = {
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": f"Recent history:\n{history_context}\n\nCurrent context:\n{context}\n\nClaude is asking: {question}"}
                ],
                "stream": False
            }
            
            try:
                async with session.post(f"{self.base_url}/api/chat", json=payload) as resp:
                    result = await resp.json()
                    decision = result["message"]["content"].strip()
                    self.decision_history.append(f"Q: {question} -> A: {decision}")
                    return decision
            except Exception as e:
                return ""  # Return empty to let user handle it manually


class ClaudeCodeInstance:
    """Manages a single Claude Code instance with PTY"""
    
    def __init__(self, instance_id: int, workdir: Path):
        self.instance_id = instance_id
        self.workdir = workdir
        self.master_fd = None
        self.slave_fd = None
        self.process = None
        self.output_buffer = []
        self.input_buffer = ""
        self.last_output = ""
        self.waiting_for_input = False
        
    def start(self):
        """Start Claude Code instance with PTY"""
        # Create PTY
        self.master_fd, self.slave_fd = pty.openpty()
        
        # Make the PTY non-blocking
        import fcntl
        flags = fcntl.fcntl(self.master_fd, fcntl.F_GETFL)
        fcntl.fcntl(self.master_fd, fcntl.F_SETFL, flags | os.O_NONBLOCK)
        
        # Set terminal size
        import struct
        winsize = struct.pack("HHHH", 40, 120, 0, 0)  # Larger terminal
        fcntl.ioctl(self.slave_fd, termios.TIOCSWINSZ, winsize)
        
        # Start Claude process without initial prompt
        env = os.environ.copy()
        
        self.process = subprocess.Popen(
            ["claude"],
            stdin=self.slave_fd,
            stdout=self.slave_fd,
            stderr=self.slave_fd,
            cwd=str(self.workdir),
            env=env,
            preexec_fn=os.setsid
        )
        
        # Close slave FD in parent
        os.close(self.slave_fd)
        
    def read_output(self) -> Optional[str]:
        """Read available output from the instance"""
        if not self.master_fd:
            return None
            
        try:
            # Check if data is available
            r, _, _ = select.select([self.master_fd], [], [], 0)
            if r:
                data = os.read(self.master_fd, 4096)
                if data:
                    output = data.decode('utf-8', errors='replace')
                    self.output_buffer.append(output)
                    self.last_output = output
                    
                    # Check if Claude is waiting for input
                    if any(indicator in output.lower() for indicator in 
                          ['(y/n)', 'yes/no', 'continue?', 'proceed?', 'confirm', '?']):
                        self.waiting_for_input = True
                    
                    return output
        except OSError:
            pass
        return None
    
    def send_input(self, text: str):
        """Send input to the Claude instance"""
        if self.master_fd:
            try:
                os.write(self.master_fd, (text + "\n").encode('utf-8'))
                self.waiting_for_input = False
            except OSError:
                pass
    
    def is_alive(self) -> bool:
        """Check if the process is still running"""
        if self.process:
            return self.process.poll() is None
        return False
    
    def terminate(self):
        """Terminate the instance"""
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
        if self.master_fd:
            os.close(self.master_fd)


class InstanceTab(TabPane):
    """Tab pane for a Claude instance"""
    
    def __init__(self, instance: ClaudeCodeInstance, decision_maker: OllamaDecisionMaker):
        super().__init__(f"Instance {instance.instance_id}", id=f"instance-{instance.instance_id}")
        self.instance = instance
        self.decision_maker = decision_maker
        self.output_area = TextArea(read_only=True)
        self.input_area = TextArea("")
        self.auto_decision_task = None
        
    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label(f"Claude Instance {self.instance.instance_id} - {self.instance.workdir}")
            with Container(classes="output-container"):
                yield self.output_area
            yield Label("Input (Enter to send, Ctrl+D for auto-decision mode):")
            yield self.input_area
    
    async def update_output(self):
        """Update output from Claude instance"""
        output = self.instance.read_output()
        if output:
            # Append to output area
            current = self.output_area.text
            self.output_area.text = current + output
            # Scroll to bottom
            self.output_area.scroll_end()
            
            # Check if we need to make a decision
            if self.instance.waiting_for_input and self.auto_decision_task:
                await self.make_auto_decision()
    
    async def make_auto_decision(self):
        """Use Ollama to make a decision"""
        context = "\n".join(self.instance.output_buffer[-20:])  # Last 20 outputs
        question = self.instance.last_output
        
        decision = await self.decision_maker.make_decision(context, question)
        if decision:
            self.instance.send_input(decision)
            self.output_area.text += f"\n[AUTO-DECISION: {decision}]\n"
            self.output_area.scroll_end()
    
    def send_user_input(self):
        """Send user input to Claude"""
        text = self.input_area.text.strip()
        if text:
            self.instance.send_input(text)
            self.output_area.text += f"\n[USER INPUT: {text}]\n"
            self.input_area.text = ""
            self.output_area.scroll_end()


class VedaInteractive(App):
    """Interactive TUI for Veda"""
    
    CSS = """
    .output-container {
        height: 70%;
        border: solid green;
    }
    
    TextArea {
        height: 100%;
    }
    
    .input-area {
        height: 3;
        border: solid blue;
    }
    """
    
    BINDINGS = [
        Binding("left", "prev_tab", "Previous Instance"),
        Binding("right", "next_tab", "Next Instance"),
        Binding("ctrl+d", "toggle_auto", "Toggle Auto-Decision"),
        Binding("ctrl+c", "quit", "Quit"),
        Binding("ctrl+n", "new_instance", "New Instance"),
    ]
    
    def __init__(self, num_instances: int = 1):
        super().__init__()
        self.num_instances = num_instances
        self.instances: List[ClaudeCodeInstance] = []
        self.tabs: List[InstanceTab] = []
        self.decision_maker = OllamaDecisionMaker()
        self.project_root = Path.cwd()
        self.auto_mode = False
        
    def compose(self) -> ComposeResult:
        yield Header()
        yield Footer()
        
        with TabbedContent() as self.tabbed_content:
            # Create initial instances
            for i in range(self.num_instances):
                instance = self.create_instance(i)
                tab = InstanceTab(instance, self.decision_maker)
                self.instances.append(instance)
                self.tabs.append(tab)
                yield tab
    
    def create_instance(self, instance_id: int) -> ClaudeCodeInstance:
        """Create a new Claude Code instance"""
        # Determine workdir (could use worktrees here)
        workdir = self.project_root
        
        instance = ClaudeCodeInstance(instance_id, workdir)
        instance.start()
        return instance
    
    async def on_mount(self):
        """Start the update loop when mounted"""
        self.update_task = asyncio.create_task(self.update_loop())
        
    async def update_loop(self):
        """Main update loop for all instances"""
        while True:
            # Update all tabs
            for tab in self.tabs:
                await tab.update_output()
            
            # Check if instances are alive
            for i, instance in enumerate(self.instances):
                if not instance.is_alive():
                    tab = self.tabs[i]
                    tab.output_area.text += "\n[INSTANCE TERMINATED]\n"
            
            await asyncio.sleep(0.1)
    
    def get_active_tab_index(self):
        """Get the current active tab index"""
        active_id = self.tabbed_content.active
        if active_id and active_id.startswith("instance-"):
            return int(active_id.split("-")[1])
        return 0
    
    def action_prev_tab(self):
        """Switch to previous tab"""
        current = self.get_active_tab_index()
        if current > 0:
            self.tabbed_content.active = f"instance-{current - 1}"
        
    def action_next_tab(self):
        """Switch to next tab"""
        current = self.get_active_tab_index()
        if current < len(self.tabs) - 1:
            self.tabbed_content.active = f"instance-{current + 1}"
    
    def action_toggle_auto(self):
        """Toggle auto-decision mode"""
        self.auto_mode = not self.auto_mode
        current_tab = self.tabs[self.get_active_tab_index()]
        
        if self.auto_mode:
            current_tab.auto_decision_task = asyncio.create_task(self.auto_decision_loop(current_tab))
            current_tab.output_area.text += "\n[AUTO-DECISION MODE ENABLED]\n"
        else:
            if current_tab.auto_decision_task:
                current_tab.auto_decision_task.cancel()
                current_tab.auto_decision_task = None
            current_tab.output_area.text += "\n[AUTO-DECISION MODE DISABLED]\n"
    
    async def auto_decision_loop(self, tab: InstanceTab):
        """Loop for auto-decision mode"""
        while True:
            if tab.instance.waiting_for_input:
                await tab.make_auto_decision()
            await asyncio.sleep(1)
    
    def action_new_instance(self):
        """Create a new instance"""
        new_id = len(self.instances)
        instance = self.create_instance(new_id)
        tab = InstanceTab(instance, self.decision_maker)
        self.instances.append(instance)
        self.tabs.append(tab)
        self.tabbed_content.add_pane(tab)
        self.tabbed_content.active = f"instance-{new_id}"
    
    def on_text_area_changed(self, event):
        """Handle input area changes"""
        active_index = self.get_active_tab_index()
        if active_index < len(self.tabs) and event.text_area == self.tabs[active_index].input_area:
            if event.text_area.text.endswith("\n"):
                self.tabs[active_index].send_user_input()
    
    async def on_unmount(self):
        """Clean up when unmounting"""
        if hasattr(self, 'update_task'):
            self.update_task.cancel()
        
        # Terminate all instances
        for instance in self.instances:
            instance.terminate()



def main():
    """Main entry point"""
    import argparse
    parser = argparse.ArgumentParser(description="Veda Interactive - Multi-instance Claude Code orchestrator")
    parser.add_argument("-n", "--instances", type=int, default=1, help="Number of instances to start (default: 1)")
    
    args = parser.parse_args()
    
    num_instances = max(1, args.instances)
    
    app = VedaInteractive(num_instances)
    app.run()


if __name__ == "__main__":
    main()