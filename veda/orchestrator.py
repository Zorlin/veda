#!/usr/bin/env python3
"""
Veda - Orchestrator for multiple Claude Code instances
Uses Ollama with deepseek-r1:14b for coordination and Task Master AI for collaboration
"""
import argparse
import asyncio
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Dict, Optional
import aiohttp
import yaml


class OllamaCoordinator:
    """Uses Ollama API with deepseek-r1:14b to analyze and coordinate tasks"""
    
    def __init__(self, base_url="http://localhost:11434"):
        self.base_url = base_url
        self.model = "deepseek-r1:14b"
    
    async def analyze_task(self, prompt: str, context: Dict) -> Dict:
        """Analyze a task and determine how to proceed"""
        system_prompt = """You are a task coordinator for multiple Claude Code instances.
        Analyze the given task and determine:
        1. Whether to use single or multiple Claude instances
        2. Whether to use same worktree (with Task Master AI) or separate worktrees
        3. Which MCP tools to emphasize (especially DeepWiki for research)
        4. How to break down the work if using multiple instances
        
        Respond in JSON format with:
        {
            "instances_needed": 1-10,
            "use_worktrees": true/false,
            "task_breakdown": ["task1", "task2", ...],
            "mcp_tools": ["deepwiki", "taskmaster-ai", ...],
            "coordination_strategy": "description"
        }
        """
        
        async with aiohttp.ClientSession() as session:
            payload = {
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": f"Task: {prompt}\n\nContext: {json.dumps(context)}"}
                ],
                "format": "json",
                "stream": False
            }
            
            async with session.post(f"{self.base_url}/api/chat", json=payload) as resp:
                result = await resp.json()
                return json.loads(result["message"]["content"])


class ClaudeInstance:
    """Represents a single Claude Code instance"""
    
    def __init__(self, instance_id: int, workdir: Path, shared_mode: bool = False):
        self.instance_id = instance_id
        self.workdir = workdir
        self.shared_mode = shared_mode
        self.process = None
        self.log_file = None
    
    async def start(self, prompt: str, mcp_config: Dict):
        """Start a Claude Code instance with the given prompt"""
        # Create instance-specific MCP config if needed
        instance_mcp = tempfile.NamedTemporaryFile(
            mode='w', 
            suffix='.json', 
            delete=False,
            prefix=f'mcp_instance_{self.instance_id}_'
        )
        json.dump({"mcpServers": mcp_config}, instance_mcp)
        instance_mcp.close()
        
        # Prepare environment
        env = os.environ.copy()
        env['MCP_CONFIG_FILE'] = instance_mcp.name
        
        # Build command
        cmd = [
            "claude",
            "-p", prompt,
            "--output-format", "stream-json"
        ]
        
        # Create log file
        self.log_file = open(f"veda_instance_{self.instance_id}.log", "w")
        
        # Start process
        self.process = await asyncio.create_subprocess_exec(
            *cmd,
            cwd=str(self.workdir),
            env=env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE
        )
        
        return self
    
    async def monitor(self):
        """Monitor the Claude instance output"""
        async for line in self.process.stdout:
            try:
                data = json.loads(line)
                print(f"[Instance {self.instance_id}] {data.get('content', '')}")
                if self.log_file:
                    self.log_file.write(line.decode())
                    self.log_file.flush()
            except json.JSONDecodeError:
                pass
    
    async def wait(self):
        """Wait for the instance to complete"""
        return await self.process.wait()


class VedaOrchestrator:
    """Main orchestrator for Veda"""
    
    def __init__(self):
        self.coordinator = OllamaCoordinator()
        self.instances: List[ClaudeInstance] = []
        self.project_root = Path.cwd()
    
    async def initialize_taskmaster(self):
        """Initialize Task Master AI for the project"""
        cmd = [
            "npx", "-y", "--package=task-master-ai", "task-master-ai",
            "init", "--yes", f"--project-root={self.project_root}"
        ]
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            print(f"Warning: Failed to initialize Task Master AI: {result.stderr}")
    
    def create_worktree(self, branch_name: str) -> Path:
        """Create a new git worktree"""
        worktree_dir = self.project_root.parent / f"veda-{branch_name}"
        cmd = ["git", "worktree", "add", str(worktree_dir), "-b", branch_name]
        subprocess.run(cmd, check=True)
        return worktree_dir
    
    async def run(self, prompt: str, force_instances: Optional[int] = None):
        """Run the orchestration"""
        # Get project context
        context = {
            "has_git": (self.project_root / ".git").exists(),
            "has_taskmaster": (self.project_root / "tasks" / "tasks.json").exists(),
            "mcp_tools": list(self.load_mcp_config().keys())
        }
        
        # Analyze task with Ollama
        print("🤔 Analyzing task with deepseek-r1:14b...")
        analysis = await self.coordinator.analyze_task(prompt, context)
        
        instances_needed = force_instances or analysis["instances_needed"]
        use_worktrees = analysis["use_worktrees"]
        
        print(f"\n📋 Analysis complete:")
        print(f"  - Instances needed: {instances_needed}")
        print(f"  - Use worktrees: {use_worktrees}")
        print(f"  - MCP tools: {', '.join(analysis['mcp_tools'])}")
        print(f"  - Strategy: {analysis['coordination_strategy']}")
        
        # Initialize Task Master if using shared worktree
        if not use_worktrees and instances_needed > 1:
            await self.initialize_taskmaster()
        
        # Prepare MCP config emphasizing requested tools
        mcp_config = self.load_mcp_config()
        
        # Create instances
        for i in range(instances_needed):
            if use_worktrees and i > 0:
                workdir = self.create_worktree(f"instance-{i}")
            else:
                workdir = self.project_root
            
            # Prepare instance-specific prompt
            if instances_needed > 1:
                instance_prompt = f"""You are Claude instance {i+1} of {instances_needed}.
                
Main task: {prompt}

Your specific subtask: {analysis['task_breakdown'][i] if i < len(analysis['task_breakdown']) else 'Assist with the main task'}

Coordination strategy: {analysis['coordination_strategy']}

{'Use Task Master AI (mcp__taskmaster-ai__) to coordinate with other instances.' if not use_worktrees else 'Work independently in your worktree.'}

Make liberal use of these MCP tools: {', '.join(analysis['mcp_tools'])}
Especially use DeepWiki (mcp__deepwiki__) for research and documentation lookup.
"""
            else:
                instance_prompt = f"""{prompt}

Make liberal use of these MCP tools: {', '.join(analysis['mcp_tools'])}
Especially use DeepWiki (mcp__deepwiki__) for research and documentation lookup.
"""
            
            instance = ClaudeInstance(i, workdir, shared_mode=not use_worktrees)
            await instance.start(instance_prompt, mcp_config)
            self.instances.append(instance)
        
        # Monitor all instances
        print(f"\n🚀 Started {len(self.instances)} Claude Code instance(s)\n")
        
        tasks = []
        for instance in self.instances:
            tasks.append(instance.monitor())
            tasks.append(instance.wait())
        
        await asyncio.gather(*tasks)
        
        print("\n✅ All instances completed")
    
    def load_mcp_config(self) -> Dict:
        """Load MCP configuration"""
        mcp_file = self.project_root / ".mcp.json"
        if mcp_file.exists():
            with open(mcp_file) as f:
                return json.load(f).get("mcpServers", {})
        return {}


async def main():
    parser = argparse.ArgumentParser(
        description="Veda - Multi-instance Claude Code orchestrator"
    )
    
    parser.add_argument(
        "-p", "--prompt",
        required=True,
        help="Task prompt for Claude Code instances"
    )
    
    parser.add_argument(
        "-n", "--instances",
        type=int,
        help="Force specific number of instances (1-10)"
    )
    
    parser.add_argument(
        "--analyze-only",
        action="store_true",
        help="Only show analysis without running instances"
    )
    
    args = parser.parse_args()
    
    orchestrator = VedaOrchestrator()
    
    if args.analyze_only:
        context = {
            "has_git": (Path.cwd() / ".git").exists(),
            "has_taskmaster": (Path.cwd() / "tasks" / "tasks.json").exists(),
            "mcp_tools": list(orchestrator.load_mcp_config().keys())
        }
        analysis = await orchestrator.coordinator.analyze_task(args.prompt, context)
        print(json.dumps(analysis, indent=2))
    else:
        await orchestrator.run(args.prompt, args.instances)


def main_sync():
    """Synchronous entry point for setuptools"""
    # Import here to avoid circular imports
    from .interactive import main as interactive_main
    interactive_main()


if __name__ == "__main__":
    main_sync()