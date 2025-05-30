#!/usr/bin/env python3
"""
Claude Manager - Handles streaming JSON communication with Claude Code instances
"""
import asyncio
import json
import subprocess
import tempfile
from pathlib import Path
from typing import List, Dict, Optional, Callable
from datetime import datetime
import aiohttp


class ClaudeStreamParser:
    """Parses Claude's streaming JSON output"""
    
    def __init__(self, instance_id: int, on_message: Callable):
        self.instance_id = instance_id
        self.on_message = on_message
        self.current_message = ""
        self.buffer = ""
        self.response_count = 0
        self.in_response = False
        
    async def process_line(self, line: str):
        """Process a line of streaming JSON output"""
        try:
            data = json.loads(line)
            
            # Handle different message types from Claude verbose output
            if data.get("type") == "system":
                # System messages (init, etc)
                if data.get("subtype") == "init":
                    await self.on_message(self.instance_id, "system", f"Session initialized: {data.get('session_id', 'unknown')}")
            elif data.get("type") == "assistant":
                # New assistant response starting
                if not self.in_response:
                    self.response_count += 1
                    self.in_response = True
                    if self.response_count > 1:
                        await self.on_message(self.instance_id, "new_response", f"Response #{self.response_count}")
                
                # Assistant messages contain the actual content
                message = data.get("message", {})
                content_blocks = message.get("content", [])
                
                for block in content_blocks:
                    if block.get("type") == "text":
                        text = block.get("text", "")
                        self.current_message += text
                        await self.on_message(self.instance_id, "stream", text)
                    elif block.get("type") == "tool_use":
                        # Handle tool use
                        tool_name = block.get("name", "unknown")
                        tool_id = block.get("id", "")
                        await self.on_message(self.instance_id, "tool_use", {
                            "name": tool_name,
                            "id": tool_id,
                            "input": block.get("input", {})
                        })
                
                # Check if message is complete
                if message.get("stop_reason"):
                    await self.on_message(self.instance_id, "complete", self.current_message)
                    self.current_message = ""
                    self.in_response = False
            elif data.get("type") == "user":
                # User messages (tool results, etc)
                content = data.get("message", {}).get("content", [])
                for item in content:
                    if item.get("type") == "tool_result":
                        await self.on_message(self.instance_id, "tool_result", {
                            "tool_use_id": item.get("tool_use_id", ""),
                            "content": item.get("content", "")
                        })
            elif data.get("type") == "error":
                # Error from Claude
                await self.on_message(self.instance_id, "error", data.get("error", "Unknown error"))
            else:
                # Other message types
                await self.on_message(self.instance_id, "data", data)
                
        except json.JSONDecodeError:
            # Not JSON, might be plain text output
            if line.strip():
                await self.on_message(self.instance_id, "text", line)


class ClaudeInstance:
    """Manages a single Claude Code instance with streaming JSON"""
    
    def __init__(self, instance_id: int, workdir: Path, on_message: Callable):
        self.instance_id = instance_id
        self.workdir = workdir
        self.process = None
        self.parser = ClaudeStreamParser(instance_id, on_message)
        self.conversation_history = []
        self.is_processing = False
        
    async def send_message(self, message: str, continue_session: bool = True):
        """Send a message to Claude using stream-json format"""
        if self.is_processing:
            return
            
        self.is_processing = True
        self.conversation_history.append({"role": "user", "content": message})
        
        # Reset parser response count for new message
        self.parser.response_count = 0
        self.parser.in_response = False
        
        # Create temporary file for the prompt
        with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
            f.write(message)
            prompt_file = f.name
        
        try:
            # Run claude command with streaming JSON output
            cmd = [
                "claude",
                "-p", message,
                "--output-format", "stream-json",
                "--verbose"
            ]
            
            # Add continue flag if this isn't the first message
            if continue_session and len(self.conversation_history) > 1:
                cmd.append("-c")
            
            self.process = await asyncio.create_subprocess_exec(
                *cmd,
                cwd=str(self.workdir),
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE
            )
            
            # Read streaming output
            async for line in self.process.stdout:
                if line:
                    await self.parser.process_line(line.decode('utf-8').strip())
            
            # Wait for process to complete
            await self.process.wait()
            
        finally:
            self.is_processing = False
            # Clean up temp file
            Path(prompt_file).unlink(missing_ok=True)
    
    def is_busy(self) -> bool:
        """Check if instance is currently processing"""
        return self.is_processing
    
    async def terminate(self):
        """Terminate the instance"""
        if self.process and self.process.returncode is None:
            self.process.terminate()
            try:
                await asyncio.wait_for(self.process.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                self.process.kill()


class OllamaAutomation:
    """Handles automated decision making with Ollama"""
    
    def __init__(self, base_url="http://localhost:11434"):
        self.base_url = base_url
        self.model = "deepseek-r1:14b"
        
    async def should_respond(self, context: str, claude_output: str) -> Optional[str]:
        """Determine if we should automatically respond and what to say"""
        system_prompt = """You are helping manage multiple Claude Code instances.
        Analyze Claude's output and determine if an automatic response is appropriate.
        
        Respond with JSON in this format:
        {
            "should_respond": true/false,
            "response": "what to say" or null,
            "reasoning": "why this decision"
        }
        
        Auto-respond when:
        - Claude asks for confirmation (y/n, yes/no)
        - Claude asks to proceed with file operations
        - Claude needs simple clarification
        - Claude is waiting for input to continue
        
        Don't auto-respond when:
        - Major architectural decisions needed
        - Claude asks open-ended questions
        - User intervention seems important
        """
        
        async with aiohttp.ClientSession() as session:
            payload = {
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": f"Context:\n{context}\n\nClaude's latest output:\n{claude_output}"}
                ],
                "format": "json",
                "stream": False
            }
            
            try:
                async with session.post(f"{self.base_url}/api/chat", json=payload) as resp:
                    result = await resp.json()
                    decision = json.loads(result["message"]["content"])
                    
                    if decision.get("should_respond"):
                        return decision.get("response")
                    return None
            except Exception as e:
                return None


class ClaudeOrchestrator:
    """Orchestrates multiple Claude instances"""
    
    def __init__(self, on_instance_message: Callable):
        self.instances: Dict[int, ClaudeInstance] = {}
        self.on_instance_message = on_instance_message
        self.automation = OllamaAutomation()
        self.auto_mode = False
        
    def create_instance(self, instance_id: int, workdir: Optional[Path] = None) -> ClaudeInstance:
        """Create a new Claude instance"""
        if workdir is None:
            workdir = Path.cwd()
            
        instance = ClaudeInstance(
            instance_id, 
            workdir,
            self._handle_instance_message
        )
        self.instances[instance_id] = instance
        return instance
    
    async def _handle_instance_message(self, instance_id: int, msg_type: str, content: any):
        """Handle messages from Claude instances"""
        # Forward to UI
        await self.on_instance_message(instance_id, msg_type, content)
        
        # Check if we should auto-respond
        if self.auto_mode and msg_type == "complete":
            instance = self.instances.get(instance_id)
            if instance:
                # Get conversation context
                context = "\n".join([
                    f"{msg['role']}: {msg['content'][:200]}..." 
                    for msg in instance.conversation_history[-5:]
                ])
                
                # Check if we should respond
                auto_response = await self.automation.should_respond(context, content)
                if auto_response:
                    # Add a small delay to make it feel more natural
                    await asyncio.sleep(1)
                    await instance.send_message(auto_response)
                    await self.on_instance_message(
                        instance_id, 
                        "auto_response", 
                        f"[AUTO] {auto_response}"
                    )
    
    async def send_to_instance(self, instance_id: int, message: str):
        """Send a message to a specific instance"""
        instance = self.instances.get(instance_id)
        if instance and not instance.is_busy():
            await instance.send_message(message)
    
    def set_auto_mode(self, enabled: bool):
        """Enable/disable automatic response mode"""
        self.auto_mode = enabled
    
    async def shutdown(self):
        """Shutdown all instances"""
        tasks = []
        for instance in self.instances.values():
            tasks.append(instance.terminate())
        if tasks:
            await asyncio.gather(*tasks)