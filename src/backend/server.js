const express = require('express');
const http = require('http');
const WebSocket = require('ws');
const path = require('path');
const cors = require('cors');
const bodyParser = require('body-parser');

// Initialize express app
const app = express();
const server = http.createServer(app);
const wss = new WebSocket.Server({ server });

// Middleware
app.use(cors());
app.use(bodyParser.json());
app.use(express.static(path.join(__dirname, '../../web')));

// Store active agents
const agents = {
  // Example: 'developer': 'running'
};

// WebSocket connections
const clients = new Set();

// WebSocket server
wss.on('connection', (ws) => {
  clients.add(ws);
  
  // Send initial agent status
  ws.send(JSON.stringify({
    type: 'agent_status',
    agents: agents
  }));
  
  ws.on('message', (message) => {
    try {
      const data = JSON.parse(message);
      // Handle incoming WebSocket messages if needed
      console.log('Received message:', data);
    } catch (error) {
      console.error('Error parsing WebSocket message:', error);
    }
  });
  
  ws.on('close', () => {
    clients.delete(ws);
  });
});

// Broadcast to all connected clients
function broadcast(data) {
  clients.forEach(client => {
    if (client.readyState === WebSocket.OPEN) {
      client.send(JSON.stringify(data));
    }
  });
}

// API Routes
// Get index page
app.get('/', (req, res) => {
  res.sendFile(path.join(__dirname, '../../web/index.html'));
});

// Project goal submission
app.post('/api/project-goal', (req, res) => {
  const { goal } = req.body;
  
  if (!goal) {
    return res.status(400).json({
      status: 'error',
      message: 'Project goal is required'
    });
  }
  
  // Here you would initialize the project with the goal
  // This is a placeholder for the actual implementation
  console.log(`Initializing project with goal: ${goal}`);
  
  // Update agent status
  agents['planner'] = 'running';
  
  // Broadcast status update
  broadcast({
    type: 'agent_status',
    agents: agents
  });
  
  res.json({
    status: 'success',
    message: 'Project initialized'
  });
});

// Chat message submission
app.post('/api/chat', (req, res) => {
  const { message, agent } = req.body;
  
  if (!message || !agent) {
    return res.status(400).json({
      status: 'error',
      message: 'Message and agent are required'
    });
  }
  
  // Here you would send the message to the specified agent
  // This is a placeholder for the actual implementation
  console.log(`Sending message to ${agent}: ${message}`);
  
  // Broadcast the message to all clients
  broadcast({
    type: 'agent_output',
    role: agent,
    text: `Received: ${message}\nThis is a placeholder response.`
  });
  
  res.json({
    status: 'success',
    message: 'Message sent'
  });
});

// Get agent status
app.get('/api/status', (req, res) => {
  res.json({
    status: 'success',
    agents: agents
  });
});

// Spawn a new agent
app.post('/api/spawn-agent', (req, res) => {
  const { role, model } = req.body;
  
  if (!role) {
    return res.status(400).json({
      status: 'error',
      message: 'Agent role is required'
    });
  }
  
  // Here you would spawn a new agent
  // This is a placeholder for the actual implementation
  console.log(`Spawning agent with role: ${role}, model: ${model || 'default'}`);
  
  // Update agent status
  agents[role] = 'running';
  
  // Broadcast status update
  broadcast({
    type: 'agent_status',
    agents: agents
  });
  
  res.json({
    status: 'success',
    message: `Agent ${role} spawned`
  });
});

// Stop all agents
app.post('/api/stop-agents', (req, res) => {
  // Here you would stop all agents
  // This is a placeholder for the actual implementation
  console.log('Stopping all agents');
  
  // Clear agent status
  Object.keys(agents).forEach(agent => {
    agents[agent] = 'stopped';
    
    // Broadcast agent exited message
    broadcast({
      type: 'agent_exited',
      role: agent
    });
  });
  
  res.json({
    status: 'success',
    message: 'All agents stopped'
  });
});

// Start the server
const PORT = process.env.PORT || 3000;
server.listen(PORT, () => {
  console.log(`Server running on port ${PORT}`);
});

module.exports = { app, server };
