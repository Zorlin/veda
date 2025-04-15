import os
import json
import logging
from pathlib import Path
from aiohttp import web

logger = logging.getLogger(__name__)

async def handle_index(request):
    """Serve the main index.html page."""
    project_root = Path(__file__).parent.parent
    index_path = project_root / "web" / "index.html"
    return web.FileResponse(index_path)

async def handle_project_goal(request, agent_manager):
    """Handle submission of a project goal."""
    try:
        data = await request.json()
        goal = data.get("goal")
        
        if not goal:
            return web.json_response({"status": "error", "message": "No goal provided"}, status=400)
        
        await agent_manager.initialize_project(goal)
        return web.json_response({"status": "success"})
    except Exception as e:
        logger.error(f"Error handling project goal: {e}")
        return web.json_response({"status": "error", "message": str(e)}, status=500)

async def handle_chat_message(request, agent_manager):
    """Handle chat messages sent to agents."""
    try:
        data = await request.json()
        message = data.get("message")
        agent = data.get("agent", "veda")  # Default to veda if no agent specified
        
        if not message:
            return web.json_response({"status": "error", "message": "No message provided"}, status=400)
        
        await agent_manager.send_to_agent(agent, message)
        return web.json_response({"status": "success"})
    except Exception as e:
        logger.error(f"Error handling chat message: {e}")
        return web.json_response({"status": "error", "message": str(e)}, status=500)

async def handle_agent_status(request, agent_manager):
    """Get the status of all agents."""
    try:
        status = agent_manager.get_agent_status()
        return web.json_response({"status": "success", "agents": status})
    except Exception as e:
        logger.error(f"Error getting agent status: {e}")
        return web.json_response({"status": "error", "message": str(e)}, status=500)

async def handle_spawn_agent(request, agent_manager):
    """Spawn a new agent with the specified role and model."""
    try:
        data = await request.json()
        role = data.get("role")
        model = data.get("model")
        initial_prompt = data.get("initial_prompt")
        
        if not role:
            return web.json_response({"status": "error", "message": "No role provided"}, status=400)
        
        await agent_manager.spawn_agent(role, model, initial_prompt)
        return web.json_response({"status": "success"})
    except Exception as e:
        logger.error(f"Error spawning agent: {e}")
        return web.json_response({"status": "error", "message": str(e)}, status=500)

async def handle_stop_agents(request, agent_manager):
    """Stop all running agents."""
    try:
        await agent_manager.stop_all_agents()
        return web.json_response({"status": "success"})
    except Exception as e:
        logger.error(f"Error stopping agents: {e}")
        return web.json_response({"status": "error", "message": str(e)}, status=500)

async def handle_websocket(request, agent_manager):
    """Handle WebSocket connections for real-time communication."""
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    
    try:
        async for msg in ws:
            if msg.type == web.WSMsgType.TEXT:
                data = json.loads(msg.data)
                
                if data.get("type") == "message":
                    # Handle chat message
                    message = data.get("data")
                    agent = data.get("agent", "veda")
                    
                    await agent_manager.send_to_agent(agent, message)
                    await ws.send_json({"status": "success", "type": "message_sent"})
                
                elif data.get("type") == "spawn_agent":
                    # Handle agent spawning
                    role = data.get("role")
                    model = data.get("model")
                    
                    await agent_manager.spawn_agent(role, model)
                    await ws.send_json({"status": "success", "type": "agent_spawned"})
            
            elif msg.type == web.WSMsgType.ERROR:
                logger.error(f"WebSocket error: {ws.exception()}")
    
    finally:
        return ws

def create_web_app(agent_manager):
    """Create and configure the web application."""
    app = web.Application()
    
    # Add routes
    app.router.add_get("/", handle_index)
    app.router.add_get("/ws", lambda request: handle_websocket(request, agent_manager))
    app.router.add_post("/api/project", lambda request: handle_project_goal(request, agent_manager))
    app.router.add_post("/api/chat", lambda request: handle_chat_message(request, agent_manager))
    app.router.add_get("/api/status", lambda request: handle_agent_status(request, agent_manager))
    app.router.add_post("/api/spawn", lambda request: handle_spawn_agent(request, agent_manager))
    app.router.add_post("/api/stop", lambda request: handle_stop_agents(request, agent_manager))
    
    # Serve static files
    project_root = Path(__file__).parent.parent
    app.router.add_static("/static", project_root / "web" / "static")
    
    return app

async def start_web_server(app, agent_manager, config):
    """Start the web server."""
    host = config.get("api", {}).get("host", "localhost")
    port = config.get("api", {}).get("port", 9900)
    
    runner = web.AppRunner(app)
    await runner.setup()
    
    site = web.TCPSite(runner, host, port)
    await site.start()
    
    logger.info(f"Web server started at http://{host}:{port}")
    
    # Keep the server running
    try:
        while True:
            await asyncio.sleep(3600)  # Sleep for an hour
    except asyncio.CancelledError:
        logger.info("Web server shutting down")
        await runner.cleanup()
