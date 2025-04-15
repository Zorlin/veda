import os
import sys
import json
import signal
import asyncio
import logging
from pathlib import Path
from unittest.mock import MagicMock

# Add project root to path
project_root = Path(__file__).parent.parent
sys.path.insert(0, str(project_root))

from src.tui import VedaApp
from src.agent_manager import AgentManager
from src.config import load_config
from src.web_server import create_web_app, start_web_server

logger = logging.getLogger(__name__)

# Constants
PID_FILE = project_root / "veda.pid"
CONFIG_FILE = project_root / "config.yaml"

def save_config(config):
    """Save configuration to the config file."""
    import yaml
    with open(CONFIG_FILE, 'w') as f:
        yaml.dump(config, f)

async def start_command():
    """Start the Veda service and web server."""
    # Check if already running
    if os.path.exists(PID_FILE):
        with open(PID_FILE, 'r') as f:
            pid = int(f.read().strip())
        
        try:
            # Check if process is running
            os.kill(pid, 0)
            print(f"Veda is already running (PID: {pid})")
            return
        except OSError:
            # Process not running, remove stale PID file
            os.remove(PID_FILE)
    
    # Load configuration
    config = load_config(CONFIG_FILE)
    
    # Create app and agent manager
    app = VedaApp(config)
    work_dir = project_root / "work"
    os.makedirs(work_dir, exist_ok=True)
    
    agent_manager = AgentManager(app, config, work_dir)
    
    # Create web app
    web_app = create_web_app(agent_manager)
    
    # Start web server
    web_server_task = asyncio.create_task(
        start_web_server(web_app, agent_manager, config)
    )
    
    # Save PID
    with open(PID_FILE, 'w') as f:
        f.write(str(os.getpid()))
    
    # Run the app
    try:
        await app.run()
    finally:
        # Clean up
        await agent_manager.stop_all_agents()
        web_server_task.cancel()
        
        try:
            await web_server_task
        except asyncio.CancelledError:
            pass
        
        if os.path.exists(PID_FILE):
            os.remove(PID_FILE)

async def chat_command():
    """Start a chat session with Veda."""
    # Load configuration
    config = load_config(CONFIG_FILE)
    
    # Create app and agent manager
    app = VedaApp(config)
    work_dir = project_root / "work"
    os.makedirs(work_dir, exist_ok=True)
    
    agent_manager = AgentManager(app, config, work_dir)
    
    print("Chat session with Veda started. Type your messages (Ctrl+D to detach, Ctrl+C to quit).")
    
    try:
        while True:
            try:
                message = input("> ")
                await agent_manager.send_to_agent("veda", message)
            except EOFError:
                # Handle Ctrl+D (detach)
                print("\nDetaching from interactive session. Veda will continue in the background.")
                if hasattr(agent_manager, 'handle_user_detach'):
                    await agent_manager.handle_user_detach()
                break
    except KeyboardInterrupt:
        # Handle Ctrl+C (quit)
        print("\nEnding chat session and stopping Veda.")
        await agent_manager.stop_all_agents()

async def stop_command():
    """Stop the Veda service."""
    if not os.path.exists(PID_FILE):
        print("Veda is not running.")
        return
    
    with open(PID_FILE, 'r') as f:
        pid = int(f.read().strip())
    
    try:
        # Send SIGTERM to the process
        os.kill(pid, signal.SIGTERM)
        print(f"Sent stop signal to Veda (PID: {pid})")
        
        # Load configuration
        config = load_config(CONFIG_FILE)
        
        # Create agent manager to stop any running agents
        app = MagicMock()
        work_dir = project_root / "work"
        agent_manager = AgentManager(app, config, work_dir)
        
        # Stop all agents
        await agent_manager.stop_all_agents()
        
        # Remove PID file
        os.remove(PID_FILE)
    except OSError:
        print(f"No process with PID {pid} found. Removing stale PID file.")
        os.remove(PID_FILE)

async def set_instances_command(args):
    """Set the number of Aider instances Veda manages."""
    if not args:
        print("Usage: veda set instances <number|auto>")
        return
    
    # Load configuration
    config = load_config(CONFIG_FILE)
    
    # Update configuration
    if args[0].lower() == "auto":
        config.setdefault("agents", {})["max_instances"] = "auto"
        print("Set Aider instances to automatic mode.")
    else:
        try:
            num_instances = int(args[0])
            if num_instances < 1:
                print("Number of instances must be at least 1.")
                return
            
            config.setdefault("agents", {})["max_instances"] = num_instances
            print(f"Set maximum Aider instances to {num_instances}.")
        except ValueError:
            print("Invalid number. Usage: veda set instances <number|auto>")
            return
    
    # Save configuration
    save_config(config)

async def status_command():
    """Display Veda's status."""
    # Check if running
    if os.path.exists(PID_FILE):
        with open(PID_FILE, 'r') as f:
            pid = int(f.read().strip())
        
        try:
            # Check if process is running
            os.kill(pid, 0)
            print(f"Veda is running (PID: {pid})")
            
            # Load configuration
            config = load_config(CONFIG_FILE)
            
            # Display configuration
            print("\nConfiguration:")
            print(f"  Web UI: http://{config.get('api', {}).get('host', 'localhost')}:{config.get('api', {}).get('port', 9900)}")
            
            max_instances = config.get("agents", {}).get("max_instances", "auto")
            print(f"  Max Aider instances: {max_instances}")
            
            # Try to get agent status
            try:
                import aiohttp
                import asyncio
                
                async def get_status():
                    async with aiohttp.ClientSession() as session:
                        async with session.get(f"http://{config.get('api', {}).get('host', 'localhost')}:{config.get('api', {}).get('port', 9900)}/api/status") as response:
                            if response.status == 200:
                                data = await response.json()
                                return data.get("agents", {})
                            return {}
                
                status = asyncio.run(get_status())
                
                if status:
                    print("\nAgent Status:")
                    for agent, state in status.items():
                        print(f"  {agent}: {state}")
            except Exception as e:
                print(f"\nCould not retrieve agent status: {e}")
            
        except OSError:
            print("Veda is not running (stale PID file found).")
            os.remove(PID_FILE)
    else:
        print("Veda is not running.")

async def help_command():
    """Display help information."""
    print("Veda - AI-Powered Software Development")
    print("\nCommands:")
    print("  veda                  Display help and status information")
    print("  veda start            Start the Veda service and web UI")
    print("  veda chat             Start a text-based chat session with Veda")
    print("  veda stop             Stop the Veda service")
    print("  veda status           Display Veda's status")
    print("  veda set instances <number|auto>  Set the maximum number of Aider instances")
    print("  veda help             Display this help information")
    print("\nWeb UI:")
    print("  The web interface is the primary way to interact with Veda.")
    print("  By default, it's available at http://localhost:9900")
    print("\nDetaching:")
    print("  Press Ctrl+D during a chat session to detach while Veda continues working.")
    print("  Press Ctrl+C to end the session and stop Veda.")

async def main():
    """Main CLI entry point."""
    # Configure logging
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
        handlers=[
            logging.FileHandler(project_root / "veda.log"),
            logging.StreamHandler()
        ]
    )
    
    # Parse command
    if len(sys.argv) < 2:
        # No command provided, show help and status
        await help_command()
        print("\n")
        await status_command()
        return
    
    command = sys.argv[1].lower()
    
    if command == "start":
        await start_command()
    elif command == "chat":
        await chat_command()
    elif command == "stop":
        await stop_command()
    elif command == "status":
        await status_command()
    elif command == "help":
        await help_command()
    elif command == "set" and len(sys.argv) >= 3 and sys.argv[2].lower() == "instances":
        await set_instances_command(sys.argv[3:])
    else:
        print(f"Unknown command: {command}")
        await help_command()

if __name__ == "__main__":
    asyncio.run(main())
