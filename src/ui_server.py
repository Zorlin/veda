import asyncio
import asyncio
import json
import logging
import websockets
# Use the modern import path if available, otherwise fallback might be needed
# from websockets.legacy.server import WebSocketServerProtocol
from websockets.server import ServerProtocol # More modern approach often uses ServerProtocol directly or via serve context
from typing import Set, Dict, Any, Optional, Tuple, List, Union
from http import HTTPStatus
from pathlib import Path
import anyio # Import anyio for TASK_STATUS_IGNORED

logger = logging.getLogger(__name__)

class UIServer:
    """Handles WebSocket connections and broadcasts harness status updates."""

    def __init__(self, host: str = "localhost", port: int = 8765):
        self.host = host
        self.port = port
        self.clients: Set[ServerProtocol] = set() # Updated type hint
        self.server_task: Optional[asyncio.Task] = None # Task for the running server
        self.loop: Optional[asyncio.AbstractEventLoop] = None # Store the loop the server runs in
        self.stop_event = asyncio.Event()
        self.latest_status: Dict[str, Any] = {"status": "Initializing", "run_id": None, "iteration": 0, "log": []}
        # Define the path to the UI directory relative to this file's location or project root
        # Assuming the script runs from the project root or src/ui_server.py location allows finding ui/
        self.ui_dir = Path(__file__).parent.parent / "ui"
        if not self.ui_dir.is_dir():
             # Fallback if running from a different structure (e.g., tests)
             self.ui_dir = Path.cwd() / "ui"
        # Note: UI serving path is still relevant for finding index.html in main.py's HTTP server
        logger.info(f"WebSocket Server initialized (host={host}, port={port})") 
        # Removed reference to serving UI from here

    # Removed _process_request method as this server will only handle WebSockets

    async def _register(self, websocket: ServerProtocol): # Updated type hint
        """Register a new client WebSocket connection."""
        self.clients.add(websocket)
        logger.info(f"Client connected: {websocket.remote_address}")
        # Send the latest status immediately upon connection
        try:
            await websocket.send(json.dumps(self.latest_status))
        except websockets.exceptions.ConnectionClosedOK:
            logger.info(f"Client {websocket.remote_address} disconnected before receiving initial status.")
        except Exception as e:
            logger.error(f"Error sending initial status to {websocket.remote_address}: {e}")


    async def _unregister(self, websocket: ServerProtocol): # Updated type hint
        """Unregister a client connection."""
        self.clients.remove(websocket)
        logger.info(f"Client disconnected: {websocket.remote_address}")

    async def _handler(self, websocket: ServerProtocol): # Removed unused 'path' parameter
        """Handle incoming WebSocket connections and messages."""
        # Registration is now handled after _process_request returns None
        await self._register(websocket)
        try:
            # Keep the WebSocket connection open and listen for messages
            async for message in websocket:
                # Handle incoming messages
                try:
                    data = json.loads(message)
                    command = data.get("command")

                    # Handle interrupt command from UI
                    if command == "interrupt" and hasattr(self, 'harness_instance') and self.harness_instance:
                        user_message = data.get("message", "")
                        interrupt_now = data.get("interrupt_now", False) # Get the interrupt flag from UI
                        log_level = logging.WARNING if interrupt_now else logging.INFO
                        logger.log(log_level, f"Received guidance from UI (Interrupt: {interrupt_now}): '{user_message[:100]}...'")
                        # Call the harness method, passing the message and the interrupt flag
                        self.harness_instance.request_interrupt(user_message, interrupt_now=interrupt_now)
                    elif command: # Log other commands if received
                         logger.info(f"Received command '{command}' from {websocket.remote_address}: {message}")
                    else: # Log non-command messages
                         logger.info(f"Received message from {websocket.remote_address}: {message}")

                except json.JSONDecodeError:
                    logger.error(f"Received invalid JSON from {websocket.remote_address}: {message}")
                except Exception as e:
                    logger.error(f"Error processing message from {websocket.remote_address}: {e}")

        except websockets.exceptions.ConnectionClosedError as e:
             logger.warning(f"Connection closed uncleanly with {websocket.remote_address}: {e}")
        except websockets.exceptions.ConnectionClosedOK:
             logger.info(f"Connection closed cleanly with {websocket.remote_address}")
        except Exception as e:
            logger.error(f"Error in WebSocket handler for {websocket.remote_address}: {e}")
        finally:
            await self._unregister(websocket)

    async def broadcast(self, message: Dict[str, Any]):
        """Broadcast a JSON message to all connected clients."""
        if not self.clients:
            return

        # Determine message content based on type
        message_type = message.get("type")
        if message_type in ["aider_output", "aider_output_clear"]:
            # For specific types, send the message dictionary directly
            message_to_send = message
            log_preview = f"type={message_type}"
            if message_type == "aider_output":
                chunk = message.get('chunk', '')
                log_preview += f", chunk={chunk[:50]}..." if chunk else ", chunk=<empty>"
            logger.debug(f"Broadcasting specific message type to {len(self.clients)} clients: {log_preview}")
        else:
            # For general status updates, update latest_status and send that
            self.latest_status.update(message)
            # Keep log history manageable
            if "log_entry" in message:
                self.latest_status["log"].append(message["log_entry"])
                self.latest_status["log"] = self.latest_status["log"][-100:]
                # Remove temporary key if it exists in the original message,
                # but don't delete from latest_status as it's part of the log array now.
                # del message["log_entry"] # No, don't delete from original message dict
            # Ensure log_entry key doesn't persist at the top level of latest_status if it came in message
            if "log_entry" in self.latest_status:
                 del self.latest_status["log_entry"]

            message_to_send = self.latest_status
            log_preview = json.dumps(message_to_send)[:200]
            logger.debug(f"Broadcasting status update to {len(self.clients)} clients: {log_preview}...")

        message_json = json.dumps(message_to_send)

        # Use asyncio.gather to send messages concurrently
        results = await asyncio.gather(
            *[client.send(message_json) for client in self.clients],
            return_exceptions=True # Don't let one failed send stop others
        )

        # Handle clients that disconnected during send
        disconnected_clients = []
        for i, result in enumerate(results):
            client = list(self.clients)[i] # Get corresponding client
            if isinstance(result, websockets.exceptions.ConnectionClosed):
                logger.warning(f"Client {client.remote_address} disconnected during broadcast. Removing.")
                disconnected_clients.append(client)
            elif isinstance(result, Exception):
                logger.error(f"Error sending message to {client.remote_address}: {result}")
                # Optionally remove clients with persistent errors
                # disconnected_clients.append(client)

        # Remove disconnected clients after iteration
        for client in disconnected_clients:
            if client in self.clients:
                 await self._unregister(client)


    async def start(self, *, task_status=anyio.TASK_STATUS_IGNORED):
        """Start the HTTP/WebSocket server, trying port+1 if needed.
        
        Accepts task_status for compatibility with anyio.TaskGroup.start().
        """
        self.stop_event.clear()
        self.loop = asyncio.get_running_loop() # Capture the loop we are running in
        current_port = self.port
        max_attempts = 2 # Try original port and port + 1
        server = None

        for attempt in range(max_attempts):
            try:
                logger.info(f"Attempting to start server on ws://{self.host}:{current_port} (Attempt {attempt + 1}/{max_attempts})")
                # Start WebSocket server without HTTP request processing
                server = await websockets.serve(
                    self._handler,
                    self.host,
                    current_port,
                    # process_request=_process_request, # Removed HTTP handling
                    ping_interval=20, # Keep connections alive
                    ping_timeout=20
                )
                self.port = current_port # Update port if successful
                logger.info(f"WebSocket server started successfully on ws://{self.host}:{self.port}")
                break # Exit loop on success
            except OSError as e:
                if "Address already in use" in str(e) and attempt < max_attempts - 1:
                    logger.warning(f"Port {current_port} is already in use. Trying port {current_port + 1}.")
                    current_port += 1
                else:
                    logger.error(f"Failed to start WebSocket server on {self.host}:{current_port}: {e}")
                    logger.error("Check if the port is already in use or if you have permissions.")
                    return # Exit start method if failed
            except Exception as e:
                 logger.exception(f"An unexpected error occurred during server startup: {e}")
                 return # Exit start method if failed

        if server is None:
             logger.error("Server could not be started after multiple attempts.")
             return

        # Signal that the server has started successfully (for TaskGroup.start)
        task_status.started()

        # Keep the server running until stop() is called
        try:
            await self.stop_event.wait()
        finally:
            logger.info("Stop event received or server task cancelled, shutting down WebSocket server...")
            server.close()
            await server.wait_closed()
            logger.info("WebSocket server stopped.")


    def stop(self):
        """Signal the server to stop."""
        logger.info("Signaling WebSocket server to stop...")
        self.stop_event.set()

    # Method to be called by the Harness to send updates
    def send_update(self, update_data: Dict[str, Any]):
        """Send an update to all connected UI clients via the server's event loop."""
        if self.loop and self.loop.is_running():
            # Schedule the broadcast coroutine in the server's event loop
            asyncio.run_coroutine_threadsafe(self.broadcast(update_data), self.loop)
        else:
             logger.warning("UI server loop not running or not found, cannot send update.")

# --- Example Usage (for testing) ---
async def main_test():
    logging.basicConfig(level=logging.DEBUG, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    server = UIServer()
    # Start the server in the background
    server_task = asyncio.create_task(server.start())

    # Give server time to start
    await asyncio.sleep(2)

    # Simulate sending updates
    logger.info("Sending test update 1")
    server.send_update({"status": "Running Aider", "iteration": 1})
    await asyncio.sleep(2)
    logger.info("Sending test update 2")
    server.send_update({"status": "Running Pytest", "iteration": 1, "log_entry": "Pytest started..."})
    await asyncio.sleep(2)
    logger.info("Sending test update 3")
    server.send_update({"status": "Evaluating", "iteration": 1, "log_entry": "Pytest failed."})
    await asyncio.sleep(5) # Keep server running longer

    logger.info("Stopping test server")
    server.stop()
    await server_task # Wait for server task to finish cleanly

if __name__ == "__main__":
    # This block is intended for running the main_test function for standalone testing.
    # The previous lines were duplicated and incorrect test code.
    logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    try:
        asyncio.run(main_test())
    except KeyboardInterrupt:
        logger.info("Test server stopped manually.")
