import threading
import time # Added import
import logging
import webbrowser
import os
import json
from flask import Flask, send_from_directory, jsonify, render_template_string, request
import os # Added import
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
    # Determine static and template folder paths relative to this file
    src_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(src_dir)
    webui_dir = os.path.join(project_root, "webui") # Assuming webui is at project root

    if not os.path.isdir(webui_dir):
        logging.warning(f"Web UI directory not found at {webui_dir}. Serving minimal UI.")
        # Fallback to minimal inline HTML if webui directory doesn't exist
        app = Flask(__name__)
    else:
        app = Flask(__name__, static_folder=webui_dir, template_folder=webui_dir)


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
            """, 500

        # Check if index.html exists in the template folder
        index_path = os.path.join(app.template_folder, 'index.html')
        if os.path.exists(index_path):
             # Serve index.html from the webui directory
             # Flask automatically looks in the template_folder for render_template
             # However, for a static SPA, sending the file might be more direct
             # return send_from_directory(app.template_folder, 'index.html')
             # Let's try rendering it as a template first, in case it uses Jinja
             try:
                 return render_template_string(open(index_path).read())
             except Exception as e:
                 logging.error(f"Error rendering {index_path}: {e}")
                 return "Error loading UI. Check logs.", 500
        else:
            # Serve a minimal Vue.js + Tailwind app inline if index.html is missing
            logging.warning("webui/index.html not found. Serving minimal inline UI.")
            html = """
            <!DOCTYPE html>
            <html lang="en">
            <head>
              <meta charset="UTF-8" />
              <title>Veda Web UI (Minimal)</title>
              <script src="https://cdn.jsdelivr.net/npm/vue@3/dist/vue.global.prod.js"></script>
              <script src="https://cdn.tailwindcss.com"></script>
            </head>
            <body class="bg-gray-100">
              <div id="app" class="max-w-4xl mx-auto mt-10 p-4 bg-white rounded shadow">
                <h1 class="text-2xl font-bold mb-4">Veda Web Interface</h1>
                <div v-if="apiKeyMissing" class="text-red-600 font-bold mb-4">
                  Error: OPENROUTER_API_KEY environment variable not set. Agents cannot run.
                </div>
                <div>
                  <h2 class="text-lg font-semibold mb-2">Chat</h2>
                  <div class="border rounded p-2 mb-4" style="min-height:3em;">Chat UI coming soon...</div>
                </div>
                <div>
                  <h2 class="text-lg font-semibold mb-2">Active Agents</h2>
                  <ul v-if="threads.length > 0">
                    <li v-for="thread in threads" :key="thread.id" class="mb-2 p-2 border rounded bg-gray-50">
                      <div class="flex justify-between items-center">
                        <span class="font-bold text-blue-700">ID: {{ thread.id }} | Role: {{ thread.role }}</span>
                        <span :class="statusClass(thread.status)" class="px-2 py-1 rounded text-sm font-semibold">{{ thread.status }}</span>
                      </div>
                      <div class="text-sm text-gray-600">Model: {{ thread.model }}</div>
                      <details class="mt-1 text-xs">
                        <summary class="cursor-pointer text-gray-500">Output Preview</summary>
                        <pre class="mt-1 p-1 bg-gray-200 rounded overflow-auto max-h-32"><code>{{ thread.output_preview.join('\\n') || 'No output yet.' }}</code></pre>
                      </details>
                    </li>
                  </ul>
                   <p v-else class="text-gray-500">No active agents.</p>
                </div>
              </div>
              <script src="/socket.io/socket.io.js"></script>
              <script>
                const { createApp, ref, onMounted } = Vue;
                const app = createApp({
                  setup() {
                    const threads = ref([]);
                    const apiKeyMissing = ref(!'{{ OPENROUTER_API_KEY or '' }}'); // Check key status

                    const socket = io();

                    const fetchThreads = () => {
                      fetch('/api/threads')
                        .then(r => r.json())
                        .then(data => { threads.value = data; })
                        .catch(err => console.error('Error fetching threads:', err));
                    };

                    const statusClass = (status) => {
                      if (status === 'running') return 'bg-green-200 text-green-800';
                      if (status.startsWith('finished')) return 'bg-blue-200 text-blue-800';
                      if (status.startsWith('failed') || status.startsWith('error')) return 'bg-red-200 text-red-800';
                      if (status.startsWith('waiting')) return 'bg-yellow-200 text-yellow-800';
                      if (status.startsWith('handoff')) return 'bg-purple-200 text-purple-800';
                      return 'bg-gray-200 text-gray-800';
                    };

                    onMounted(() => {
                      fetchThreads(); // Initial load

                      // Listen for updates from server
                      socket.on('connect', () => {
                        console.log('Socket connected');
                      });
                      socket.on('disconnect', () => {
                        console.log('Socket disconnected');
                      });
                      socket.on('threads_update', (updatedThreads) => {
                        console.log('Received threads update:', updatedThreads);
                        threads.value = updatedThreads;
                      });
                      socket.on('error', (error) => {
                        console.error('Socket error:', error);
                      });
                    });

                    return { threads, apiKeyMissing, statusClass };
                  }
                });
                app.mount('#app');
              </script>
            </body>
            </html>
            """
            return html

    @app.route("/api/threads")
    def api_threads():
        """Returns the state of active agents from the manager instance."""
        if agent_manager_instance:
            agents_data = agent_manager_instance.get_active_agents_status()
            return jsonify(agents_data)
        else:
            logging.error("AgentManager instance not available for /api/threads")
            return jsonify({"error": "AgentManager not initialized"}), 500

    return app

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

    flask_app = create_flask_app()
    # Combine Flask app with SocketIO
    wsgi_app = socketio.WSGIApp(sio, flask_app)

    def run_server():
        logging.info(f"Starting web server at http://{host}:{port}")
        try:
            # Use Werkzeug's run_simple to host the combined WSGI app
            run_simple(host, port, wsgi_app, use_reloader=False, use_debugger=False)
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
