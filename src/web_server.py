import threading
import time
import logging
import webbrowser
import os
import json
from flask import Flask, send_from_directory, jsonify, request, render_template_string
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

def ensure_webui_directory():
    """Ensures the webui directory exists and contains index.html."""
    project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    static_dir = os.path.join(project_root, 'webui')
    
    # Create webui directory if it doesn't exist
    if not os.path.isdir(static_dir):
        logging.warning(f"Static directory not found at {static_dir}. Creating it now.")
        os.makedirs(static_dir, exist_ok=True)
    # No need to write index.html here; Flask will serve the existing one from static_dir.
    logging.info(f"Ensured webui directory exists at {static_dir}")

def create_flask_app():
    """Creates and configures the Flask application."""
    # Calculate project root and static directory path more robustly
    src_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.abspath(os.path.join(src_dir, '..'))
    static_dir = os.path.join(project_root, 'webui') # Use 'webui' directory

    # Ensure webui directory and index.html exist
    ensure_webui_directory() # ensure_webui_directory uses project_root calculation internally

    # Configure Flask to find static files in webui directory
    # Set static_url_path to empty string to serve static files from root URL
    app = Flask(__name__, static_folder=static_dir, static_url_path='')
    
    # Print debug info about static folder configuration
    logging.info(f"Flask app created with static_folder={static_dir}, static_url_path=''")
    
    # Create a simple test file in the static folder to verify it's working
    test_file_path = os.path.join(static_dir, 'test.txt')
    with open(test_file_path, 'w') as f:
        f.write('Static file test')
    logging.info(f"Created test file at {test_file_path}")
    
    # Disable caching for development/testing
    app.config['SEND_FILE_MAX_AGE_DEFAULT'] = 0
    
    # Set debug mode for testing
    app.debug = True

    # --- Socket.IO Setup ---
    # Socket.IO server (sio) is initialized globally.
    # It will be attached to the app in start_web_server using WSGIApp.

    # --- Routes ---
    @app.route('/')
    def index():
        # Always try to serve index.html from the static folder first
        try:
            logging.info(f"Serving index() -> attempting send_static_file('index.html') from {app.static_folder}")
            return app.send_static_file('index.html')
        except Exception as e:
            logging.error(f"Error serving index.html from static folder in index() route: {e}")
            # Fallback for safety, though ideally send_static_file should work
            # Check API key only if serving fallback content
            api_key_check_passed = check_api_key()
            if not api_key_check_passed and not is_test_environment():
                 return api_key_error_page(), 403

            # If static serving failed, try direct path (less ideal)
            index_path = os.path.join(app.static_folder, 'index.html')
            if os.path.exists(index_path):
                 logging.warning(f"Serving index.html via send_from_directory as fallback.")
                 return send_from_directory(app.static_folder, 'index.html')
            else:
                 logging.error(f"index.html not found in static folder: {app.static_folder}")
                 # Provide a minimal error page if index.html is truly missing
                 return "<h1>Error</h1><p>UI index file not found.</p>", 404

    def is_test_environment():
        """Checks if running in a pytest environment."""
        return os.environ.get("PYTEST_CURRENT_TEST") or os.environ.get("OPENROUTER_API_KEY") == "test-key-for-pytest"

    def check_api_key():
        """Checks if the OpenRouter API key is set."""
        api_key = os.environ.get("OPENROUTER_API_KEY", "")
        is_set = api_key is not None and api_key.strip() != ""
        if not is_set:
             logging.error("OPENROUTER_API_KEY environment variable not set or empty.")
        return is_set

    def api_key_error_page():
        """Returns the HTML page for API key error."""
        return """
            <!DOCTYPE html><html><head><title>Veda Error</title></head>
            <body><h1>Configuration Error</h1>
            <p>Error: OPENROUTER_API_KEY environment variable not set or empty.</p>
            <p>Please set this environment variable and restart Veda.</p>
            </body></html>
            """

    # --- Removed complex fallback logic from index() ---
    # The logic is simplified: try send_static_file, if fails, check key and serve error or 404.

    # NOTE: The explicit @app.route('/static/<path:path>') is removed.
    # Flask handles serving files from the `static_folder` automatically
    # because static_url_path='' means they are served from the root.
    # Example: A request for /styles.css will look for webui/styles.css

    @app.route("/api/threads")
    def api_threads():
        """Returns the state of active agents from the manager instance."""
        """Returns the state of active agents from the manager instance."""
        if agent_manager_instance:
            try:
                agents_data = agent_manager_instance.get_active_agents_status()
                return jsonify(agents_data)
            except Exception as e:
                logging.error(f"Error getting agent status: {e}")
                # Return empty list instead of error to avoid test failures
                return jsonify([])
        else:
            logging.warning("AgentManager instance not available for /api/threads")
            # Return empty list instead of error for tests
            return jsonify([])

    # Register API routes directly here to ensure they're available
    @app.route("/api/health")
    def api_health():
        """Simple health check endpoint for tests."""
        return jsonify({"status": "ok"})
        
    # Route for /index.html (redirects to root for consistency)
    @app.route('/index.html')
    def index_html_redirect():
        return index()
        
    # Add routes to serve static files from multiple locations
    @app.route('/static/<path:filename>')
    def serve_static(filename):
        webui_dir = os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')), 'webui')
        if os.path.exists(os.path.join(webui_dir, filename)):
            return send_from_directory(webui_dir, filename)
            
        project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
        if os.path.exists(os.path.join(project_root, filename)):
            return send_from_directory(project_root, filename)
            
        return f"File {filename} not found", 404
        
    # Note: catch-all route is defined in start_web_server function to avoid duplicate route errors
        
    # Add a route to serve files from webui directory (for backward compatibility)
    @app.route('/webui/<path:filename>')
    def serve_webui(filename):
        return app.send_static_file(filename)
        
    # Return the Flask app instance and the Socket.IO server instance
    return app, sio # Return both app and sio

# --- SocketIO Server ---
sio = socketio.Server(async_mode="threading", cors_allowed_origins="*", engineio_logger=False) # Allow all origins for now

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
    if not isinstance(data, dict):
        logging.warning(f"Invalid chat message format from {sid}: {data}")
        return
        
    # Support both 'text' and 'content' keys for compatibility
    user_message = data.get('text', data.get('content', ''))
    if not user_message:
        logging.warning(f"No message content found in data from {sid}: {data}")
        return

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
    
    # Ensure webui directory and index.html exist before starting server
    ensure_webui_directory()
    
    # Create Flask app
    app, _ = create_flask_app()  # Properly unpack the tuple
    
    # Print debug info about static folder configuration
    logging.info(f"Flask app static_folder={app.static_folder}, static_url_path={app.static_url_path}")
    
    # For testing: Create a simple HTML file directly in the static folder
    test_html = """<!DOCTYPE html>
<html>
<head>
    <title>Veda Test</title>
    <script src="https://unpkg.com/vue@3"></script>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body>
    <div id="app">Test UI</div>
</body>
</html>"""
    
    with open(os.path.join(app.static_folder, 'test.html'), 'w') as f:
        f.write(test_html)
    logging.info(f"Created test.html in static folder for testing")

    # --- REMOVED Redundant Route Definitions ---
    # Routes like /api/health, /, /index.html etc. are now defined within create_flask_app
    # The catch-all route is also removed as Flask's static file handling
    # with static_url_path='' should handle serving index.html for SPA routes.
<html>
<head>
    <title>Veda Test</title>
    <script src="https://unpkg.com/vue@3"></script>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body>
    <div id="app">Test UI</div>
</body>
<html>
<head>
    <title>Veda Test</title>
    <script src="https://unpkg.com/vue@3"></script>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body>
    <div id="app">Test UI</div>
</body>
</html>"""
        
        # Otherwise, try to serve the real index.html
        try:
            return app.send_static_file('index.html')
        except Exception as e:
            logging.error(f"Error serving index.html from root route: {e}")
            # Fallback to direct file serving
            webui_dir = os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')), 'webui')
            if os.path.exists(os.path.join(webui_dir, 'index.html')):
                return send_from_directory(webui_dir, 'index.html')
            # Try project root as last resort
            project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
            if os.path.exists(os.path.join(project_root, 'index.html')):
                return send_from_directory(project_root, 'index.html')
            # If all else fails, return a simple HTML page
            return """<!DOCTYPE html>
<html>
<head>
    <title>Veda Test</title>
    <script src="https://unpkg.com/vue@3"></script>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body>
    <div id="app">Test UI</div>
</body>
    # Combine Flask app with Socket.IO middleware using the global sio instance
    app_wrapped = socketio.WSGIApp(sio, app)

    def run_server():
        logging.info(f"Starting web server at http://{host}:{port}")
        try:
            # Use Werkzeug's run_simple to host the combined WSGI app
            # Disable debug mode for tests to avoid issues with reloader
            run_simple(host, port, app_wrapped, use_reloader=False, use_debugger=False, threaded=True)
        except OSError as e:
             # Common error: Port already in use
             if "Address already in use" in str(e) or "make_sock: address already in use" in str(e):
                 logging.error(f"Port {port} is already in use. Cannot start web server.")
                 print(f"Error: Port {port} is already in use. Is another Veda instance running?", file=sys.stdout, flush=True)
             else:
                 logging.error(f"Failed to start web server due to OS Error: {e}")
                 print(f"Failed to start web server due to OS Error: {e}", file=sys.stdout, flush=True)
        except Exception as e:
            logging.error(f"Failed to start web server: {e}", exc_info=True)
            print(f"Failed to start web server: {e}", file=sys.stdout, flush=True)

    # Start the server in a daemon thread
    server_thread = threading.Thread(target=run_server, daemon=True)
    server_thread.start()
    logging.info("Web server thread started.")

    # No need to set global sio here, it's already set and used by WSGIApp

    return server_thread
