import asyncio
import json
import logging
import websockets
from websockets.server import WebSocketServerProtocol
from typing import Set, Dict, Any, Optional, Tuple, List, Union
from http import HTTPStatus
from pathlib import Path

logger = logging.getLogger(__name__)

class UIServer:
    """Handles WebSocket connections and broadcasts harness status updates."""

    def __init__(self, host: str = "localhost", port: int = 8765):
        self.host = host
        self.port = port
        self.clients: Set[WebSocketServerProtocol] = set()
        self.server_task: Optional[asyncio.Task] = None
        self.stop_event = asyncio.Event()
        self.latest_status: Dict[str, Any] = {"status": "Initializing", "run_id": None, "iteration": 0, "log": []}
        # Define the path to the UI directory relative to this file's location or project root
        # Assuming the script runs from the project root or src/ui_server.py location allows finding ui/
        self.ui_dir = Path(__file__).parent.parent / "ui"
        if not self.ui_dir.is_dir():
             # Fallback if running from a different structure (e.g., tests)
             self.ui_dir = Path.cwd() / "ui"
        logger.info(f"UI Server initialized (host={host}, port={port}), serving UI from {self.ui_dir}")


    async def _process_request(
        self, path: str, request_headers: websockets.Headers
    ) -> Optional[Tuple[HTTPStatus, List[Tuple[str, str]], bytes]]:
        """Handle HTTP requests before WebSocket handshake."""
        if path == "/" or path == "/index.html":
            html_file = self.ui_dir / "index.html"
            if html_file.is_file():
                try:
                    content = html_file.read_bytes()
                    headers = [("Content-Type", "text/html")]
                    return HTTPStatus.OK, headers, content
                except Exception as e:
                    logger.error(f"Error reading {html_file}: {e}")
                    body = b"Internal Server Error"
                    headers = [("Content-Type", "text/plain")]
                    return HTTPStatus.INTERNAL_SERVER_ERROR, headers, body
            else:
                logger.warning(f"UI file not found: {html_file}")
                body = b"Not Found"
                headers = [("Content-Type", "text/plain")]
                return HTTPStatus.NOT_FOUND, headers, body
        # Let websockets handle other paths (potential WebSocket connections)
        return None

    async def _register(self, websocket: WebSocketServerProtocol):
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


    async def _unregister(self, websocket: WebSocketServerProtocol):
        """Unregister a client connection."""
        self.clients.remove(websocket)
        logger.info(f"Client disconnected: {websocket.remote_address}")

    async def _handler(self, websocket: WebSocketServerProtocol, path: str):
        """Handle incoming WebSocket connections and messages."""
        # Registration is now handled after _process_request returns None
        await self._register(websocket)
        try:
            # Keep the WebSocket connection open and listen for messages
            async for message in websocket:
                # Handle incoming messages if needed in the future
                logger.info(f"Received message from {websocket.remote_address}: {message}")
                # Example: Process commands from UI
                # data = json.loads(message)
                # if data.get("command") == "pause": ...
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

        # Update latest status
        self.latest_status.update(message)
        # Keep log history manageable (e.g., last 100 entries)
        if "log_entry" in message:
            self.latest_status["log"].append(message["log_entry"])
            self.latest_status["log"] = self.latest_status["log"][-100:] # Keep last 100 log entries
            del self.latest_status["log_entry"] # Don't keep the temporary key

        message_json = json.dumps(self.latest_status)
        logger.debug(f"Broadcasting update to {len(self.clients)} clients: {message_json[:200]}...")

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


    async def start(self):
        """Start the WebSocket server."""
        logger.info(f"Starting WebSocket server on ws://{self.host}:{self.port}")
        self.stop_event.clear()
        try:
            # Pass the HTTP request processor
            server = await websockets.serve(
                self._handler,
                self.host,
                self.port,
                process_request=self._process_request, # Add this line
                ping_interval=20, # Keep connections alive
                ping_timeout=20
            )
            logger.info("HTTP/WebSocket server started.")
            # Keep the server running until stop() is called
            await self.stop_event.wait()
            logger.info("Stop event received, shutting down server...")
            server.close()
            await server.wait_closed()
            logger.info("WebSocket server stopped.")
        except OSError as e:
             logger.error(f"Failed to start WebSocket server on {self.host}:{self.port}: {e}")
             logger.error("Check if the port is already in use or if you have permissions.")
        except Exception as e:
            logger.exception(f"An unexpected error occurred in the WebSocket server: {e}")


    def stop(self):
        """Signal the server to stop."""
        logger.info("Signaling WebSocket server to stop...")
        self.stop_event.set()

    # Method to be called by the Harness to send updates
    def send_update(self, update_data: Dict[str, Any]):
        """Send an update to all connected UI clients."""
        # Run the broadcast in the server's event loop
        if self.server_task and not self.server_task.done():
             asyncio.run_coroutine_threadsafe(self.broadcast(update_data), self.server_task.get_loop())
        else:
             logger.warning("UI server task not running, cannot send update.")

# --- Example Usage (for testing) ---
async def main_test():
    server = UIServer()
    server.server_task = asyncio.create_task(server.start()) # Store task reference

    # Simulate sending updates
    await asyncio.sleep(5)
    server.send_update({"status": "Running Aider", "iteration": 1})
    await asyncio.sleep(5)
    server.send_update({"status": "Running Pytest", "iteration": 1, "log_entry": "Pytest started..."})
    await asyncio.sleep(5)
    server.send_update({"status": "Evaluating", "iteration": 1, "log_entry": "Pytest failed."})
    await asyncio.sleep(10)

    server.stop()
    await server.server_task # Wait for server task to finish

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    try:
        asyncio.run(main_test())
    except KeyboardInterrupt:
        logger.info("Test server stopped manually.")
