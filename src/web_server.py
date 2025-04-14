import threading
import time # Added import
import logging
import webbrowser
import os
import json
from flask import Flask, send_from_directory, jsonify, request # Removed render_template_string
# Removed redundant 'import os'
import socketio
from werkzeug.serving import run_simple

# Allow finding constants.py and agent_manager.py when run from project root
import sys
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from constants import OPENROUTER_API_KEY, VEDA_CHAT_MODEL, OLLAMA_URL
from chat import ollama_chat # Import the chat function
# Import AgentManager type hint without circular dependency during initialization
from typing import TYPE_CHECKING, List, Dict
if TYPE_CHECKING:
    from agent_manager import AgentManager


# --- Flask Web UI with Vue.js and TailwindCSS ---

# Disable Flask's default logging to avoid duplication with our setup
log = logging.getLogger('werkzeug')
log.setLevel(logging.ERROR)

# Global reference to the AgentManager instance (will be set by main.py)
# This is not ideal, dependency injection would be better, but follows current pattern.
agent_manager_instance: 'AgentManager' = None

def create_flask_app():
    """Creates and configures the Flask application."""
    # Calculate project root and static directory path
    project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    static_dir = os.path.join(project_root, 'static') # Changed from 'webui' to 'static'

    # Check if static dir exists during app creation for early feedback
    if not os.path.isdir(static_dir):
        logging.warning(f"Static directory not found at {static_dir}. Web UI might not load correctly.")
        # Proceed anyway, maybe static files aren't essential for all modes.

    # Configure Flask to find static files in ../static relative to this file's dir parent
    # Flask automatically creates the /static route based on static_folder and static_url_path
    app = Flask(__name__, static_folder=static_dir, static_url_path='/static')

    # --- Socket.IO Setup ---
    # Socket.IO server (sio) is initialized globally.
    # It will be attached to the app in start_web_server using WSGIApp.

    # --- Routes ---
    @app.route("/")
    def index():
        # Check if OPENROUTER_API_KEY is set in the environment before serving
        # Read directly from os.environ within the request context
        api_key = os.environ.get("OPENROUTER_API_KEY")
        if api_key is None or api_key.strip() == "":
            logging.error("OPENROUTER_API_KEY environment variable not set or empty in web server process.")
            return """
            <!DOCTYPE html><html><head><title>Veda Error</title></head>
            <body><h1>Configuration Error</h1>
            <p>Error: OPENROUTER_API_KEY environment variable not set or empty.</p>
            <p>Please set this environment variable and restart Veda.</p>
            </body></html>
            """, 403 # Forbidden due to config issue

        # Serve index.html from the configured static folder
        try:
            # Ensure static_folder is configured and exists before trying to serve from it
            if not app.static_folder or not os.path.isdir(app.static_folder):
                 logging.error(f"Static folder '{app.static_folder}' not configured or does not exist.")
                 return "Server configuration error: Static folder not found.", 500

            # Use Flask's send_from_directory to serve index.html from the static folder
            return send_from_directory(app.static_folder, 'index.html')
        except FileNotFoundError:
            # This exception is raised by send_from_directory if index.html is missing
            logging.error(f"index.html not found in static directory: {app.static_folder}")
            # Check if the file actually exists for more detailed logging
            expected_path = os.path.join(app.static_folder, 'index.html')
            if not os.path.exists(expected_path):
                 logging.error(f"Confirmed: File does not exist at {expected_path}")
            else:
                 # This case is unlikely but possible if permissions are wrong after check
                 logging.error(f"File may exist at {expected_path} but send_from_directory failed (Permissions?).")
            return "Error: index.html not found. Build the frontend first.", 404
        except Exception as e:
            # Catch any other unexpected errors during file serving
            logging.error(f"Error serving index.html from {app.static_folder}: {e}", exc_info=True)
            return "Error loading UI. Check logs.", 500

        # NOTE: The minimal inline UI fallback is removed as we now rely on the static file.
        # If index.html is missing, a 404 or 500 error will be returned.

    # NOTE: The explicit @app.route('/static/<path:path>') is removed.
    # Flask handles serving files from the `static_folder` automatically
    # at the `static_url_path` (which defaults to '/static' if not specified).


    @app.route("/api/threads")
    def api_threads():
        """Returns the state of active agents from the manager instance."""
        if agent_manager_instance:
            agents_data = agent_manager_instance.get_active_agents_status()
              <script src="https://cdn.jsdelivr.net/npm/vue@3/dist/vue.global.prod.js"></script>
            return jsonify(agents_data)
        else:
            logging.error("AgentManager instance not available for /api/threads")
            return jsonify({"error": "AgentManager not initialized"}), 500

    # Return the Flask app instance and the Socket.IO server instance
    return app, sio # Return both app and sio

# --- SocketIO Server ---
sio = socketio.Server(async_mode="threading", cors_allowed_origins="*") # Allow all origins for now

@sio.event
def connect(sid, environ):
    logging.info(f"Client connected: {sid}")
    # Send initial state when client connects
    if agent_manager_instance:
        try:
            initial_data = agent_manager_instance.get_active_agents_status()
            sio.emit('threads_update', initial_data, room=sid)
        except Exception as e:
            logging.error(f"Error sending initial state to client {sid}: {e}")
    else:
        logging.warning(f"AgentManager not ready when client {sid} connected.")


@sio.event
def disconnect(sid):
    logging.info(f"Client disconnected: {sid}")

# Store chat history per session (simple in-memory example)
# TODO: Persist history or integrate with a more robust chat management system
chat_histories: Dict[str, List[Dict[str, str]]] = {}

@sio.event
def chat_message(sid, data):
    """Handles incoming chat messages from a client."""
    logging.info(f"Received chat message from {sid}: {data}")
    if not isinstance(data, dict) or 'text' not in data:
        logging.warning(f"Invalid chat message format from {sid}: {data}")
        return

    user_message = data['text']

    # Get or initialize history for this session
    if sid not in chat_histories:
        chat_histories[sid] = [] # Start fresh history for new connection

    session_history = chat_histories[sid]
    session_history.append({"role": "user", "content": user_message})

    # Limit history size (optional, prevents memory issues)
    max_history = 10
    if len(session_history) > max_history * 2: # Keep last N pairs
         chat_histories[sid] = session_history[-(max_history * 2):]


    try:
        # Call the Ollama chat function
        # Use the VEDA_CHAT_MODEL and OLLAMA_URL from constants
        # Pass the current session's history
        veda_response = ollama_chat(
            messages=chat_histories[sid], # Pass history for context
            model=VEDA_CHAT_MODEL,
            api_url=OLLAMA_URL
        )

        if veda_response:
            logging.info(f"Veda response for {sid}: {veda_response}")
            # Add Veda's response to history
            chat_histories[sid].append({"role": "assistant", "content": veda_response})
            # Send response back to the specific client
            sio.emit('chat_update', {'sender': 'veda', 'text': veda_response}, room=sid)
        else:
            logging.warning(f"Received empty response from ollama_chat for {sid}")
            sio.emit('chat_update', {'sender': 'veda', 'text': "[Error: Could not get response]"}, room=sid)

    except Exception as e:
        logging.error(f"Error processing chat message for {sid}: {e}", exc_info=True)
        # Notify the user of the error
        sio.emit('chat_update', {'sender': 'veda', 'text': f"[Error: {e}]"}, room=sid)


# Function to be called by AgentManager or other components to push agent updates
def broadcast_agent_update():
    """Fetches current agent status and broadcasts it via SocketIO."""
    if agent_manager_instance and sio:
        try:
            current_data = agent_manager_instance.get_active_agents_status()
            sio.emit('threads_update', current_data)
            logging.debug("Broadcasted threads_update via SocketIO.")
        except Exception as e:
            logging.error(f"Error broadcasting agent update: {e}")
    elif not agent_manager_instance:
        logging.warning("Cannot broadcast agent update: AgentManager not initialized.")
    elif not sio:
         logging.warning("Cannot broadcast agent update: SocketIO server not initialized.")

# --- Web Server Start Function ---
def start_web_server(manager_instance: 'AgentManager', host: str = "0.0.0.0", port: int = 9900):
    """Starts the Flask-SocketIO web server in a separate thread."""
    global agent_manager_instance
    agent_manager_instance = manager_instance # Set the global instance

    app, sio_server = create_flask_app() # Get both app and sio instance

    # Combine Flask app with Socket.IO middleware for use with run_simple
    # This is the correct way for Werkzeug/Flask dev server
    app_wrapped = socketio.WSGIApp(sio_server, app)

    def run_server():
        logging.info(f"Starting web server at http://{host}:{port}")
        try:
            # Use Werkzeug's run_simple to host the combined WSGI app
            run_simple(host, port, app_wrapped, use_reloader=False, use_debugger=False, threaded=True) # Use wrapped app, ensure threaded=True
            # run_simple is blocking, so the thread will stay alive running the server.
        except OSError as e:
             # Common error: Port already in use
             if "Address already in use" in str(e) or "make_sock: address already in use" in str(e):
                 logging.error(f"Port {port} is already in use. Cannot start web server.")
                 print(f"Error: Port {port} is already in use. Is another Veda instance running?", file=sys.stdout, flush=True)
             else:
                 logging.error(f"Failed to start web server due to OS Error: {e}")
                 print(f"Failed to start web server due to OS Error: {e}", file=sys.stdout, flush=True)
        except Exception as e:
            logging.error(f"Failed to start web server: {e}", exc_info=True) # Log traceback
            print(f"Failed to start web server: {e}", file=sys.stdout, flush=True)

    # Start the server in a daemon thread so it doesn't block the main Veda process
    server_thread = threading.Thread(target=run_server, daemon=True)
    server_thread.start()
    logging.info("Web server thread started.")
    # Removed periodic broadcast - updates should be pushed when state changes
    return server_thread # Return the thread object if needed
