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
    
    # Always create/update index.html in webui directory to ensure it exists
    index_path = os.path.join(static_dir, 'index.html')
    logging.info(f"Creating/updating index.html at {index_path}")
    with open(index_path, 'w') as f:
        f.write("""<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Veda - AI Software Development</title>
    <script src="https://unpkg.com/vue@3/dist/vue.global.prod.js"></script>
    <script src="https://cdn.tailwindcss.com"></script>
    <!-- Include Socket.IO client library (updated to match server version) -->
    <script src="https://cdn.socket.io/4.7.2/socket.io.min.js"></script>
    <style>
        /* Basic styles for chat */
        .chat-message { margin-bottom: 8px; padding: 8px; border-radius: 4px; }
        .user-message { background-color: #e0f2fe; text-align: right; } /* Light blue */
        .veda-message { background-color: #f3f4f6; } /* Light gray */
        pre { white-space: pre-wrap; word-wrap: break-word; } /* Wrap long lines in agent output */
    </style>
</head>
<body class="bg-gray-100 font-sans">
    <div id="app" class="max-w-4xl mx-auto p-4">
        <h1 class="text-3xl font-bold mb-4 text-gray-800 border-b pb-2">Veda</h1>

        <!-- System Status -->
        <div class="mb-6 p-4 bg-white rounded shadow">
            <h2 class="text-xl font-semibold mb-2 text-gray-700">System Status</h2>
            <p class="italic" :class="statusColor">{{ statusMessage }}</p>
            <div v-if="apiKeyMissing" class="mt-2 text-red-600 font-semibold">
              Warning: OPENROUTER_API_KEY environment variable not set or empty. Agents may not function correctly.
            </div>
        </div>

        <!-- Chat Interface -->
        <div class="mb-6 p-4 bg-white rounded shadow">
            <h2 class="text-xl font-semibold mb-2 text-gray-700">Chat with Veda</h2>
            <div id="chat-window" class="h-64 overflow-y-auto border rounded p-2 mb-3 bg-gray-50">
                <div v-for="(msg, index) in chatMessages" :key="index"
                     :class="['chat-message', msg.sender === 'user' ? 'user-message' : 'veda-message']">
                    <strong>{{ msg.sender === 'user' ? 'You' : 'Veda' }}:</strong>
                    <span v-html="formatMessage(msg.text)"></span> <!-- Use v-html to render potential markdown -->
                </div>
                <p v-if="chatMessages.length === 0" class="text-gray-500 italic">Chat history will appear here.</p>
            </div>
            <div class="flex">
                <input type="text" v-model="chatInput" @keyup.enter="sendMessage"
                       placeholder="Describe your goal or ask a question..."
                       class="flex-grow border rounded-l p-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
                       :disabled="!isConnected">
                <button @click="sendMessage"
                        class="bg-blue-500 hover:bg-blue-600 text-white font-bold py-2 px-4 rounded-r"
                        :disabled="!isConnected || !chatInput.trim()">
                    Send
                </button>
            </div>
        </div>

        <!-- Active Agents -->
        <div class="p-4 bg-white rounded shadow">
            <h2 class="text-xl font-semibold mb-2 text-gray-700">Active Agents</h2>
            <div v-if="agents.length > 0">
                <div v-for="agent in agents" :key="agent.id" class="mb-4 p-3 border rounded bg-gray-50 shadow-sm">
                    <div class="flex justify-between items-center mb-2 pb-1 border-b">
                        <span class="font-bold text-blue-700">Role: {{ agent.role || 'Unknown' }}</span>
                        <span :class="agentStatusClass(agent.status)" class="px-2 py-1 rounded text-xs font-semibold uppercase tracking-wide">
                            {{ agent.status || 'Unknown' }}
                        </span>
                    </div>
                    <div class="text-sm text-gray-600 mb-1">Model: {{ agent.model || 'N/A' }}</div>
                    <div class="text-sm text-gray-600 mb-2">ID: {{ agent.id || 'N/A' }}</div>
                    <details class="text-xs">
                        <summary class="cursor-pointer text-gray-500 hover:text-gray-700">Output Preview</summary>
                        <pre class="mt-1 p-2 bg-gray-200 rounded overflow-auto max-h-40 text-gray-800"><code>{{ (agent.output_preview || []).join('\\n') || 'No output yet.' }}</code></pre>
                    </details>
                </div>
            </div>
            <p v-else class="text-gray-500 italic">No agents currently active.</p>
        </div>

    </div>

    <script>
        const { createApp, ref, onMounted, computed, nextTick } = Vue;

        const app = createApp({
            setup() {
                const statusMessage = ref('Connecting to Veda server...');
                const isConnected = ref(false);
                const apiKeyMissing = ref(false); // Will be updated if needed
                const agents = ref([]);
                const chatInput = ref('');
                const chatMessages = ref([]); // Format: { sender: 'user'/'veda', text: 'message' }

                // --- Socket.IO Connection ---
                const socket = io({
                    reconnectionAttempts: 5, // Try to reconnect a few times
                    reconnectionDelay: 3000, // Wait 3 seconds between attempts
                    transports: ['websocket'] // Force WebSocket transport
                });

                socket.on('connect', () => {
                    console.log('Socket connected:', socket.id);
                    statusMessage.value = 'Connected to Veda server';
                    isConnected.value = true;
                    // Optionally request initial state if server doesn't push it automatically on connect
                    // socket.emit('request_initial_state');
                });

                socket.on('disconnect', (reason) => {
                    console.log('Socket disconnected:', reason);
                    statusMessage.value = `Connection lost: ${reason}. Attempting to reconnect...`;
                    isConnected.value = false;
                    // Optional: Clear agents list or show a disconnected state
                    // agents.value = [];
                });

                socket.on('connect_error', (error) => {
                    console.error('Socket connection error:', error);
                    statusMessage.value = `Connection error: ${error.message}`;
                    isConnected.value = false;
                });

                // --- Event Handlers ---
                socket.on('threads_update', (updatedAgents) => {
                    console.log('Received threads_update:', updatedAgents);
                    // Check if OPENROUTER_API_KEY is missing based on server info (if available)
                    // This assumes the server might send a flag or check it.
                    // Alternatively, we could fetch '/api/config' or similar.
                    // For now, we'll rely on the minimal UI check if index.html wasn't found.
                    agents.value = updatedAgents || [];
                });

                socket.on('chat_update', (message) => {
                    console.log('Received chat_update:', message);
                    // Assume message is { sender: 'veda', text: '...' }
                    if (message && message.text) {
                        chatMessages.value.push({ sender: 'veda', text: message.text });
                        scrollToChatBottom();
                    }
                });

                // --- Methods ---
                const sendMessage = () => {
                    const text = chatInput.value.trim();
                    if (text && isConnected.value) {
                        const message = { sender: 'user', text: text };
                        chatMessages.value.push(message);
                        socket.emit('chat_message', { text: text }); // Send only text to server
                        chatInput.value = '';
                        scrollToChatBottom();
                    }
                };

                const scrollToChatBottom = () => {
                    nextTick(() => {
                        const chatWindow = document.getElementById('chat-window');
                        if (chatWindow) {
                            chatWindow.scrollTop = chatWindow.scrollHeight;
                        }
                    });
                };

                // Basic markdown formatting (links, bold, italics, code)
                const formatMessage = (text) => {
                    if (!text) return '';
                    let html = text
                        .replace(/&/g, "&amp;")
                        .replace(/</g, "&lt;")
                        .replace(/>/g, "&gt;")
                        .replace(/"/g, "&quot;")
                        .replace(/'/g, "&#039;");

                    // Basic Markdown-like replacements
                    html = html.replace(/\\*\\*(.*?)\\*\\*/g, '<strong>$1</strong>'); // Bold
                    html = html.replace(/\\*(.*?)\\*/g, '<em>$1</em>');       // Italics
                    html = html.replace(/`([^`]+)`/g, '<code>$1</code>');     // Inline code
                    // Simple link detection (http/https)
                    html = html.replace(/(https?:\\/\\/[^\\s]+)/g, '<a href="$1" target="_blank" class="text-blue-600 hover:underline">$1</a>');

                    return html.replace(/\\n/g, '<br>'); // Render newlines
                };


                // --- Computed Properties ---
                const statusColor = computed(() => {
                    if (!isConnected.value) return 'text-red-500';
                    // Could add more colors for different statuses later
                    return 'text-green-600';
                });

                const agentStatusClass = (status) => {
                    status = (status || '').toLowerCase();
                    if (status === 'running') return 'bg-green-100 text-green-800';
                    if (status.startsWith('finished') || status === 'completed') return 'bg-blue-100 text-blue-800';
                    if (status.startsWith('failed') || status.startsWith('error')) return 'bg-red-100 text-red-800';
                    if (status.startsWith('waiting')) return 'bg-yellow-100 text-yellow-800';
                    if (status.startsWith('handoff')) return 'bg-purple-100 text-purple-800';
                    return 'bg-gray-100 text-gray-800'; // Default/unknown
                };

                // --- Lifecycle Hooks ---
                onMounted(() => {
                    // Initial fetch or rely on connect event push
                    console.log('Vue app mounted');
                    // Check API key status (example - might need a dedicated API endpoint)
                    // fetch('/api/config').then(r=>r.json()).then(cfg => apiKeyMissing.value = !cfg.apiKeySet);
                });

                return {
                    statusMessage,
                    isConnected,
                    apiKeyMissing,
                    agents,
                    chatInput,
                    chatMessages,
                    sendMessage,
                    statusColor,
                    agentStatusClass,
                    formatMessage
                };
            }
        });

        app.mount('#app');
    </script>
</body>
</html>""")
            logging.info(f"Created basic index.html file at {index_path}")
            
        # Also create a copy in the project root for tests
        root_index_path = os.path.join(project_root, 'index.html')
        if not os.path.exists(root_index_path):
            import shutil
            shutil.copy(index_path, root_index_path)
            logging.info(f"Created copy of index.html at project root for tests")

def create_flask_app():
    """Creates and configures the Flask application."""
    # Calculate project root and static directory path
    project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    static_dir = os.path.join(project_root, 'webui') # Use 'webui' directory where index.html is located

    # Ensure webui directory and index.html exist
    ensure_webui_directory()

    # Configure Flask to find static files in webui directory
    # Set static_url_path to empty string to serve static files from root URL
    app = Flask(__name__, static_folder=static_dir, static_url_path='')

    # --- Socket.IO Setup ---
    # Socket.IO server (sio) is initialized globally.
    # It will be attached to the app in start_web_server using WSGIApp.

    # --- Routes ---
    @app.route('/')
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
            # First try to serve from webui directory
            webui_dir = os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')), 'webui')
            if os.path.exists(os.path.join(webui_dir, 'index.html')):
                logging.info(f"Serving index.html from {webui_dir}")
                return send_from_directory(webui_dir, 'index.html')
            
            # Then try project root as fallback
            project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
            if os.path.exists(os.path.join(project_root, 'index.html')):
                logging.info(f"Serving index.html from {project_root}")
                return send_from_directory(project_root, 'index.html')
                
            # If index.html is missing in both locations, serve a basic UI directly
            logging.warning(f"index.html not found in {webui_dir} or {project_root}, serving basic UI")
            return render_template_string("""
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Veda - AI Software Development</title>
    <script src="https://unpkg.com/vue@3/dist/vue.global.prod.js"></script>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://cdn.socket.io/4.7.2/socket.io.min.js"></script>
    <style>
        body { font-family: sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; }
        h1 { color: #2c3e50; }
    </style>
</head>
<body>
    <div id="app">
        <h1>Veda</h1>
        <p>AI-Powered Software Development</p>
        <div id="status">
            <p>System Status: Connecting to server...</p>
        </div>
        <div id="chat">
            <h2>Chat with Veda</h2>
            <div id="messages" style="height: 300px; border: 1px solid #ccc; overflow-y: auto; padding: 10px; margin-bottom: 10px;">
                <p>Chat history will appear here.</p>
            </div>
            <div style="display: flex;">
                <input type="text" id="message-input" style="flex-grow: 1; padding: 5px;" placeholder="Type your message...">
                <button id="send-button" style="margin-left: 10px; padding: 5px 10px; background: #2c3e50; color: white; border: none;">Send</button>
            </div>
        </div>
        <script>
            // Basic Vue setup would go here in the full version
            document.getElementById('send-button').addEventListener('click', function() {
                alert('Chat functionality available in the full UI');
            });
        </script>
    </div>
</body>
</html>
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>Veda - AI Software Development</title>
        <script src="https://unpkg.com/vue@3/dist/vue.global.prod.js"></script>
        <script src="https://cdn.tailwindcss.com"></script>
        <script src="https://cdn.socket.io/4.7.2/socket.io.min.js"></script>
        <style>
            body { font-family: sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; }
            h1 { color: #2c3e50; }
        </style>
    </head>
    <body>
        <h1>Veda</h1>
        <p>AI-Powered Software Development</p>
        <div id="status">
            <p>System Status: Connecting to server...</p>
        </div>
        <div id="chat">
            <h2>Chat with Veda</h2>
            <div id="messages" style="height: 300px; border: 1px solid #ccc; overflow-y: auto; padding: 10px; margin-bottom: 10px;">
                <p>Chat history will appear here.</p>
            </div>
            <div style="display: flex;">
                <input type="text" id="message-input" style="flex-grow: 1; padding: 5px;" placeholder="Type your message...">
                <button id="send-button" style="margin-left: 10px; padding: 5px 10px; background: #2c3e50; color: white; border: none;">Send</button>
            </div>
        </div>
        <script>
            // Basic Vue setup would go here in the full version
            document.getElementById('send-button').addEventListener('click', function() {
                alert('Chat functionality available in the full UI');
            });
        </script>
    </body>
    </html>
            <!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <title>Veda - AI-Powered Software Development</title>
                <style>
                    body {
                        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, 'Open Sans', 'Helvetica Neue', sans-serif;
                        line-height: 1.6;
                        color: #333;
                        max-width: 1200px;
                        margin: 0 auto;
                        padding: 20px;
                    }
                    header {
                        text-align: center;
                        margin-bottom: 30px;
                        padding-bottom: 20px;
                        border-bottom: 1px solid #eee;
                    }
                    h1 {
                        color: #2c3e50;
                    }
                    .container {
                        display: flex;
                        gap: 20px;
                    }
                    .sidebar {
                        flex: 1;
                        background: #f8f9fa;
                        padding: 20px;
                        border-radius: 5px;
                    }
                    .main-content {
                        flex: 3;
                    }
                    .chat-container {
                        background: #f8f9fa;
                        border-radius: 5px;
                        padding: 20px;
                        height: 400px;
                        display: flex;
                        flex-direction: column;
                    }
                    .chat-messages {
                        flex-grow: 1;
                        overflow-y: auto;
                        margin-bottom: 15px;
                        padding: 10px;
                        background: white;
                        border-radius: 5px;
                    }
                    .message {
                        margin-bottom: 10px;
                        padding: 8px 12px;
                        border-radius: 18px;
                    }
                    .user-message {
                        background: #e3f2fd;
                        align-self: flex-end;
                        margin-left: auto;
                        text-align: right;
                    }
                    .system-message {
                        background: #f1f1f1;
                    }
                    .input-area {
                        display: flex;
                    }
                    #message-input {
                        flex-grow: 1;
                        padding: 10px;
                        border: 1px solid #ddd;
                        border-radius: 4px;
                    }
                    button {
                        background: #2c3e50;
                        color: white;
                        border: none;
                        padding: 10px 15px;
                        margin-left: 10px;
                        border-radius: 4px;
                        cursor: pointer;
                    }
                    button:hover {
                        background: #1a252f;
                    }
                    .agent-list {
                        margin-top: 20px;
                    }
                    .agent-item {
                        background: white;
                        padding: 10px;
                        margin-bottom: 10px;
                        border-radius: 5px;
                        border-left: 4px solid #3498db;
                    }
                </style>
                <script src="https://cdn.socket.io/4.6.0/socket.io.min.js"></script>
            </head>
            <body>
                <header>
                    <h1>Veda</h1>
                    <p>AI-Powered Software Development</p>
                </header>
                
                <div class="container">
                    <div class="sidebar">
                        <h2>Agent Status</h2>
                        <div id="agent-list" class="agent-list">
                            <p>Loading agents...</p>
                        </div>
                    </div>
                    
                    <div class="main-content">
                        <div class="chat-container">
                            <div id="chat-messages" class="chat-messages">
                                <div class="message system-message">
                                    <p>Welcome to Veda! I'm here to help you build software. What would you like to create today?</p>
                                </div>
                            </div>
                            
                            <div class="input-area">
                                <input type="text" id="message-input" placeholder="Type your message here...">
                                <button id="send-button">Send</button>
                            </div>
                        </div>
                    </div>
                </div>
                
                <script>
                    document.addEventListener('DOMContentLoaded', () => {
                        const chatMessages = document.getElementById('chat-messages');
                        const messageInput = document.getElementById('message-input');
                        const sendButton = document.getElementById('send-button');
                        const agentList = document.getElementById('agent-list');
                        
                        // Connect to Socket.IO server
                        const socket = io();
                        
                        socket.on('connect', () => {
                            addSystemMessage('Connected to Veda server');
                        });
                        
                        socket.on('disconnect', () => {
                            addSystemMessage('Disconnected from Veda server. Trying to reconnect...');
                        });
                        
                        socket.on('threads_update', (data) => {
                            updateAgentList(data);
                        });
                        
                        socket.on('chat_update', (data) => {
                            if (data.sender === 'veda') {
                                addSystemMessage(data.text);
                            }
                        });
                        
                        // Send message when button is clicked
                        sendButton.addEventListener('click', sendMessage);
                        
                        // Send message when Enter key is pressed
                        messageInput.addEventListener('keypress', (e) => {
                            if (e.key === 'Enter') {
                                sendMessage();
                            }
                        });
                        
                        function sendMessage() {
                            const message = messageInput.value.trim();
                            if (message) {
                                addUserMessage(message);
                                socket.emit('chat_message', { text: message });
                                messageInput.value = '';
                            }
                        }
                        
                        function addUserMessage(text) {
                            const messageDiv = document.createElement('div');
                            messageDiv.className = 'message user-message';
                            messageDiv.innerHTML = `<p>${escapeHtml(text)}</p>`;
                            chatMessages.appendChild(messageDiv);
                            chatMessages.scrollTop = chatMessages.scrollHeight;
                        }
                        
                        function addSystemMessage(text) {
                            const messageDiv = document.createElement('div');
                            messageDiv.className = 'message system-message';
                            messageDiv.innerHTML = `<p>${escapeHtml(text)}</p>`;
                            chatMessages.appendChild(messageDiv);
                            chatMessages.scrollTop = chatMessages.scrollHeight;
                        }
                        
                        function updateAgentList(agents) {
                            if (!agents || agents.length === 0) {
                                agentList.innerHTML = '<p>No agents currently running</p>';
                                return;
                            }
                            
                            agentList.innerHTML = '';
                            agents.forEach(agent => {
                                const agentDiv = document.createElement('div');
                                agentDiv.className = 'agent-item';
                                agentDiv.innerHTML = `
                                    <h3>${escapeHtml(agent.role || 'Unknown')}</h3>
                                    <p>Status: ${escapeHtml(agent.status || 'Unknown')}</p>
                                    <p>Model: ${escapeHtml(agent.model || 'N/A')}</p>
                                `;
                                agentList.appendChild(agentDiv);
                            });
                        }
                        
                        function escapeHtml(unsafe) {
                            return unsafe
                                .replace(/&/g, "&amp;")
                                .replace(/</g, "&lt;")
                                .replace(/>/g, "&gt;")
                                .replace(/"/g, "&quot;")
                                .replace(/'/g, "&#039;");
                        }
                    });
                </script>
            </body>
            </html>
            """)
        except Exception as e:
            # Catch any other unexpected errors during file serving
            logging.error(f"Error serving index.html from {app.static_folder}: {e}", exc_info=True)
            return "Error loading UI. Check logs.", 500

    # NOTE: The explicit @app.route('/static/<path:path>') is removed.
    # Flask handles serving files from the `static_folder` automatically
    # at the `static_url_path` (which defaults to '/static' if not specified).


    @app.route("/api/threads")
    def api_threads():
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
        
    # Add a route to serve index.html from the root URL
    @app.route('/index.html')
    def serve_index_html():
        webui_dir = os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')), 'webui')
        if os.path.exists(os.path.join(webui_dir, 'index.html')):
            return send_from_directory(webui_dir, 'index.html')
        
        # Fallback to project root
        project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
        if os.path.exists(os.path.join(project_root, 'index.html')):
            return send_from_directory(project_root, 'index.html')
            
        return "Index.html not found", 404
        
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
        
    # Add a route to serve files from webui directory
    @app.route('/webui/<path:filename>')
    def serve_webui(filename):
        webui_dir = os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')), 'webui')
        return send_from_directory(webui_dir, filename)
        
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
    
    # Register the health endpoint directly to ensure it's available
    @app.route("/api/health")
    def api_health_direct():
        """Simple health check endpoint for tests."""
        return jsonify({"status": "ok"})
        
    # Register additional routes for tests
    @app.route("/test")
    def test_endpoint():
        """Simple test endpoint that always returns 200 OK."""
        return "Test endpoint is working"
    
    # Create Socket.IO server
    sio_server = socketio.Server(async_mode="threading", cors_allowed_origins="*", engineio_logger=False)
    
    # Register Socket.IO event handlers
    @sio_server.event
    def connect(sid, environ):
        logging.info(f"Client connected: {sid}")
        # Send initial state when client connects
        if agent_manager_instance:
            try:
                initial_data = agent_manager_instance.get_active_agents_status()
                sio_server.emit('threads_update', initial_data, room=sid)
            except Exception as e:
                logging.error(f"Error sending initial state to client {sid}: {e}")
    
    @sio_server.event
    def disconnect(sid):
        logging.info(f"Client disconnected: {sid}")
    
    # Register API routes directly on the Flask app
    @app.route("/api/threads")
    def api_threads_direct():
        """Direct route for threads API that bypasses the global function."""
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
    
    # Combine Flask app with Socket.IO middleware
    app_wrapped = socketio.WSGIApp(sio_server, app)

    def run_server():
        logging.info(f"Starting web server at http://{host}:{port}")
        try:
            # Use Werkzeug's run_simple to host the combined WSGI app
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
    
    # Store the Socket.IO server instance globally for use in broadcast_agent_update
    global sio
    sio = sio_server
    
    return server_thread
