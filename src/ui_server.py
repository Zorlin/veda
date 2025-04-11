import asyncio
import json
import logging
import websockets
from websockets.server import ServerProtocol
from typing import Set, Dict, Any, Optional, Tuple, List, Union, TYPE_CHECKING
from http import HTTPStatus
from pathlib import Path
import anyio
from anyio.streams.memory import MemoryObjectReceiveStream # Specific type hint

# Avoid circular import for type hinting Harness
if TYPE_CHECKING:
    from .harness import Harness

logger = logging.getLogger(__name__)

class UIServer:
    """Handles WebSocket connections, listens for updates, and broadcasts them."""

    def __init__(self, host: str = "localhost", port: int = 8765):
        self.host = host
        self.port = port
        self.clients: Set[ServerProtocol] = set()
        self.server_task: Optional[asyncio.Task] = None
        self.stop_event = asyncio.Event()
        self.latest_status: Dict[str, Any] = {"status": "Initializing", "run_id": None, "iteration": 0, "log": []}
        # Stream for receiving updates from Harness
        self.ui_receive_stream: Optional[MemoryObjectReceiveStream] = None
        # Reference to Harness for sending interrupts back
        self.harness_instance: Optional['Harness'] = None
        # Define the path to the UI directory relative to this file's location or project root
        # Assuming the script runs from the project root or src/ui_server.py location allows finding ui/
        self.ui_dir = Path(__file__).parent.parent / "ui"
        if not self.ui_dir.is_dir():
             # Fallback if running from a different structure (e.g., tests)
             self.ui_dir = Path.cwd() / "ui"
        # Note: UI serving path is still relevant for finding index.html in main.py's HTTP server
        logger.info(f"WebSocket Server initialized (host={host}, port={port})")

    def set_harness_instance(self, harness_instance: 'Harness'):
        """Allows main script to inject the Harness instance for callbacks."""
        self.harness_instance = harness_instance

    def set_receive_stream(self, receive_stream: MemoryObjectReceiveStream):
        """Allows main script to inject the receive stream."""
        self.ui_receive_stream = receive_stream

    async def _register(self, websocket: ServerProtocol):
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
                    if command == "interrupt" and self.harness_instance: # Check if harness_instance is set
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


    async def _update_listener(self):
        """Listen for updates from the Harness via the receive stream."""
        if not self.ui_receive_stream:
            logger.error("Receive stream not set. Cannot start update listener.")
            return

        logger.info("Update listener started.")
        try:
            while not self.stop_event.is_set():
                try:
                    # Directly await receiving from the stream
                    update = await self.ui_receive_stream.receive()
                    # Broadcast the update to all connected clients
                    await self.broadcast(update)
                except (anyio.EndOfStream, anyio.ClosedResourceError): # Handle stream closure gracefully
                    logger.info("End of stream reached. Stopping update listener.")
                    break
                except Exception as e:
                    if not self.stop_event.is_set():  # Only log if not stopping intentionally
                        logger.error(f"Error in update listener: {e}")
                    break
        except Exception as e:
            logger.exception(f"Unexpected error in update listener: {e}")
        finally:
            logger.info("Update listener stopped.")

    async def start(self, *, task_status=anyio.TASK_STATUS_IGNORED):
        """Start the HTTP/WebSocket server, trying port+1 if needed.
        
        Accepts task_status for compatibility with anyio.TaskGroup.start().
        """
        if not self.ui_receive_stream:
            logger.error("Receive stream not set. Cannot start UI server listener.")
            # Signal failure if using task_status
            if task_status is not anyio.TASK_STATUS_IGNORED:
                 # Need a way to signal error back if using start_soon context
                 # For now, just log and return, preventing server start.
                 # A more robust solution might involve raising an exception
                 # that the caller (main.py) can catch.
                 pass # Or raise RuntimeError("Receive stream not set")
            return

        self.stop_event.clear()
        current_port = self.port
        max_attempts = 2 # Try original port and port + 1
        websocket_server = None

        async def serve_websocket(task_status=anyio.TASK_STATUS_IGNORED):
            """Inner function to start the websocket server."""
            nonlocal websocket_server, current_port # Allow modification of outer scope variables
            srv = None
            for attempt in range(max_attempts):
                try:
                    logger.info(f"Attempting to start WebSocket server on ws://{self.host}:{current_port} (Attempt {attempt + 1}/{max_attempts})")
                    srv = await websockets.serve(
                        self._handler,
                        self.host,
                        current_port,
                        # process_request=_process_request, # Removed HTTP handling
                        ping_interval=20, # Keep connections alive
                        ping_timeout=20
                    )
                    # If websockets.serve() succeeds without raising an exception, break the loop.
                    break # Exit loop on success
                except OSError as e:
                    if "Address already in use" in str(e) and attempt < max_attempts - 1:
                        logger.warning(f"Port {current_port} is already in use. Trying port {current_port + 1}.")
                        current_port += 1
                    else:
                        logger.error(f"Failed to start WebSocket server on {self.host}:{current_port} due to OSError: {e}")
                        # Let the loop finish, srv will remain None
                        break # Exit loop on final OSError failure
                except RuntimeError as e:
                    # Catch RuntimeErrors during startup
                    logger.exception(f"An unexpected RuntimeError occurred during server startup attempt on port {current_port}: {e}")
                    srv = None # Ensure srv is None after this kind of error
                    break # Exit loop, cannot proceed
                except Exception as e:
                    # Catch any other unexpected error during startup
                    logger.exception(f"An unexpected error occurred during server startup attempt on port {current_port}: {e}")
                    srv = None # Ensure srv is None after this kind of error
                    break # Exit loop

            # --- End of for loop ---

            # Check if the loop completed successfully (i.e., srv is not None)
            if srv is None:
                 logger.error("Server could not be started after all attempts or due to an error.")
                 # Do NOT call task_status.started() - signal failure to the TaskGroup by returning.
                 # The TaskGroup will raise the "child exited without calling task_status.started()" error.
                 return # Exit serve_websocket

            # --- Server started successfully ---
            self.port = current_port # Update port to the one that worked
            logger.info(f"WebSocket server started successfully on ws://{self.host}:{self.port}")
            websocket_server = srv # Store the server object

            # Signal that the server has started successfully (for TaskGroup.start)
            task_status.started()

            # Keep the server running until stop() is called
            try:
                await self.stop_event.wait()
            finally:
                logger.info("Stop event received or server task cancelled, shutting down WebSocket server...")
                # Use the correct variable name holding the server object
                if websocket_server:
                    websocket_server.close()
                    await websocket_server.wait_closed()
                logger.info("WebSocket server stopped.")

        # Start the server and listener tasks concurrently
        try:
            async with anyio.create_task_group() as tg:
                # Start the WebSocket server task, waiting for it to signal readiness
                await tg.start(serve_websocket)
                # Start the update listener task concurrently
                tg.start_soon(self._update_listener)
                logger.info("WebSocket server and update listener tasks started.")
                # Signal overall readiness if required by the caller context
                task_status.started()
                # The group will now wait until all tasks complete or are cancelled.
                # The stop_event mechanism inside serve_websocket handles shutdown.

        except Exception as e:
             logger.exception(f"Error in UI server main task group: {e}")
        finally:
             logger.info("UI Server start method finished.")


    def stop(self):
        """Signal the server to stop."""
        logger.info("Signaling WebSocket server and listener to stop...")
        self.stop_event.set()
        # Close the stream from the server side as well to unblock listener if waiting
        if self.ui_receive_stream:
            self.ui_receive_stream.close()


    # Removed send_update method - updates now come via stream


# --- Example Usage (for testing) ---
async def main_test():
    logging.basicConfig(level=logging.DEBUG, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    # Create a dummy stream pair for testing
    send_stream, receive_stream = anyio.create_memory_object_stream(float('inf'))

    server = UIServer()
    server.set_receive_stream(receive_stream) # Inject the stream

    async with anyio.create_task_group() as tg:
        # Start the server in the background using the task group
        await tg.start(server.start)
        logger.info("Test UI Server started.")

        # Give server time to start
        await anyio.sleep(1)

        # Simulate sending updates from another "thread" (task) via the stream
        logger.info("Simulating sending test update 1")
        await send_stream.send({"status": "Running Aider", "iteration": 1})
        await anyio.sleep(1)
        logger.info("Simulating sending test update 2")
        await send_stream.send({"status": "Running Pytest", "iteration": 1, "log_entry": "Pytest started..."})
        await anyio.sleep(1)
        logger.info("Simulating sending test update 3")
        await send_stream.send({"status": "Evaluating", "iteration": 1, "log_entry": "Pytest failed."})
        await anyio.sleep(3) # Keep server running longer

        logger.info("Stopping test server")
        server.stop()
        # Closing the send stream also signals the end to the receiver
        await send_stream.aclose()
        # Task group cancellation will be handled by server.stop() triggering stop_event

if __name__ == "__main__":
    # This block is intended for running the main_test function for standalone testing.
    # The previous lines were duplicated and incorrect test code.
    logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(name)s - %(message)s")
    try:
        asyncio.run(main_test())
    except KeyboardInterrupt:
        logger.info("Test server stopped manually.")
