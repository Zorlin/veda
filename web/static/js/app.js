document.addEventListener('DOMContentLoaded', function() {
    // DOM elements
    const goalInput = document.getElementById('goal-input');
    const submitGoalBtn = document.getElementById('submit-goal');
    const projectGoalSection = document.getElementById('project-goal');
    const agentTabsSection = document.getElementById('agent-tabs');
    const agentControlsSection = document.getElementById('agent-controls');
    const tabHeader = document.querySelector('.tab-header');
    const tabContent = document.querySelector('.tab-content');
    const statusList = document.getElementById('status-list');
    const spawnAgentBtn = document.getElementById('spawn-agent');
    const spawnForm = document.getElementById('spawn-form');
    const confirmSpawnBtn = document.getElementById('confirm-spawn');
    const cancelSpawnBtn = document.getElementById('cancel-spawn');
    const stopAgentsBtn = document.getElementById('stop-agents');
    
    // WebSocket connection
    let socket;
    let statusInterval;
    
    // Initialize WebSocket connection
    function initWebSocket() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws`;
        
        socket = new WebSocket(wsUrl);
        
        socket.onopen = function() {
            console.log('WebSocket connection established');
        };
        
        socket.onmessage = function(event) {
            const data = JSON.parse(event.data);
            
            if (data.type === 'agent_output') {
                addMessage(data.role, data.text, false);
            } else if (data.type === 'agent_status') {
                updateAgentStatus(data.agents);
            } else if (data.type === 'agent_exited') {
                updateAgentStatus({ [data.role]: 'exited' });
            }
        };
        
        socket.onclose = function() {
            console.log('WebSocket connection closed');
            // Try to reconnect after a delay
            setTimeout(initWebSocket, 3000);
        };
        
        socket.onerror = function(error) {
            console.error('WebSocket error:', error);
        };
    }
    
    // Submit project goal
    submitGoalBtn.addEventListener('click', function() {
        const goal = goalInput.value.trim();
        
        if (!goal) {
            alert('Please enter a project goal');
            return;
        }
        
        fetch('/api/project', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({ goal })
        })
        .then(response => response.json())
        .then(data => {
            if (data.status === 'success') {
                // Show agent tabs and controls
                projectGoalSection.classList.add('hidden');
                agentTabsSection.classList.remove('hidden');
                agentControlsSection.classList.remove('hidden');
                
                // Add initial message
                addMessage('veda', `I'll help you build: ${goal}`, false);
                
                // Start polling for agent status
                getAgentStatus();
                statusInterval = setInterval(getAgentStatus, 5000);
            } else {
                alert('Error: ' + (data.message || 'Failed to submit project goal'));
            }
        })
        .catch(error => {
            console.error('Error:', error);
            alert('Failed to submit project goal. Please try again.');
        });
    });
    
    // Send chat message
    document.addEventListener('click', function(e) {
        if (e.target.classList.contains('send-button')) {
            const chatPanel = e.target.closest('.agent-chat');
            const agent = chatPanel.dataset.agent;
            const inputElement = chatPanel.querySelector('.chat-input');
            const message = inputElement.value.trim();
            
            if (!message) return;
            
            // Add message to UI
            addMessage(agent, message, true);
            
            // Clear input
            inputElement.value = '';
            
            // Send message to server
            fetch('/api/chat', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ message, agent })
            })
            .then(response => response.json())
            .then(data => {
                if (data.status !== 'success') {
                    alert('Error: ' + (data.message || 'Failed to send message'));
                }
            })
            .catch(error => {
                console.error('Error:', error);
                alert('Failed to send message. Please try again.');
            });
        }
    });
    
    // Switch tabs
    tabHeader.addEventListener('click', function(e) {
        if (e.target.classList.contains('tab-button')) {
            const agent = e.target.dataset.agent;
            
            // Update active tab button
            document.querySelectorAll('.tab-button').forEach(btn => {
                btn.classList.remove('active');
            });
            e.target.classList.add('active');
            
            // Update active chat panel
            document.querySelectorAll('.agent-chat').forEach(panel => {
                panel.classList.remove('active');
            });
            document.querySelector(`.agent-chat[data-agent="${agent}"]`).classList.add('active');
        }
    });
    
    // Spawn agent form toggle
    spawnAgentBtn.addEventListener('click', function() {
        spawnForm.classList.toggle('hidden');
    });
    
    // Cancel spawn
    cancelSpawnBtn.addEventListener('click', function() {
        spawnForm.classList.add('hidden');
    });
    
    // Confirm spawn agent
    confirmSpawnBtn.addEventListener('click', function() {
        const role = document.getElementById('agent-role').value;
        const model = document.getElementById('agent-model').value;
        
        fetch('/api/spawn', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({ role, model })
        })
        .then(response => response.json())
        .then(data => {
            if (data.status === 'success') {
                spawnForm.classList.add('hidden');
                alert(`Agent ${role} spawned successfully`);
                
                // Add tab for new agent if it doesn't exist
                if (!document.querySelector(`.tab-button[data-agent="${role}"]`)) {
                    addAgentTab(role);
                }
                
                // Update agent status
                getAgentStatus();
            } else {
                alert('Error: ' + (data.message || 'Failed to spawn agent'));
            }
        })
        .catch(error => {
            console.error('Error:', error);
            alert('Failed to spawn agent. Please try again.');
        });
    });
    
    // Stop all agents
    stopAgentsBtn.addEventListener('click', function() {
        if (confirm('Are you sure you want to stop all agents?')) {
            fetch('/api/stop', {
                method: 'POST'
            })
            .then(response => response.json())
            .then(data => {
                if (data.status === 'success') {
                    alert('All agents stopped successfully');
                    getAgentStatus();
                } else {
                    alert('Error: ' + (data.message || 'Failed to stop agents'));
                }
            })
            .catch(error => {
                console.error('Error:', error);
                alert('Failed to stop agents. Please try again.');
            });
        }
    });
    
    // Add message to chat
    function addMessage(agent, text, isUser) {
        const chatPanel = document.querySelector(`.agent-chat[data-agent="${agent}"]`);
        
        // If chat panel doesn't exist for this agent, create it
        if (!chatPanel && !isUser) {
            addAgentTab(agent);
        }
        
        const messagesContainer = document.querySelector(`.agent-chat[data-agent="${agent}"] .chat-messages`);
        
        const messageElement = document.createElement('div');
        messageElement.classList.add('message');
        messageElement.classList.add(isUser ? 'user-message' : 'agent-message');
        
        // Format code blocks
        text = formatCodeBlocks(text);
        
        messageElement.innerHTML = text;
        messagesContainer.appendChild(messageElement);
        
        // Scroll to bottom
        messagesContainer.scrollTop = messagesContainer.scrollHeight;
    }
    
    // Format code blocks in messages
    function formatCodeBlocks(text) {
        // Replace ```language\ncode\n``` with formatted code blocks
        return text.replace(/```(\w*)\n([\s\S]*?)\n```/g, function(match, language, code) {
            return `<pre class="code-block ${language}"><code>${escapeHtml(code)}</code></pre>`;
        });
    }
    
    // Escape HTML to prevent XSS
    function escapeHtml(text) {
        return text
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#039;");
    }
    
    // Add a new agent tab
    function addAgentTab(agent) {
        // Add tab button
        const tabButton = document.createElement('button');
        tabButton.classList.add('tab-button');
        tabButton.dataset.agent = agent;
        tabButton.textContent = agent.charAt(0).toUpperCase() + agent.slice(1);
        tabHeader.appendChild(tabButton);
        
        // Add chat panel
        const chatPanel = document.createElement('div');
        chatPanel.classList.add('agent-chat');
        chatPanel.dataset.agent = agent;
        
        chatPanel.innerHTML = `
            <div class="chat-messages"></div>
            <div class="input-container">
                <textarea class="chat-input" placeholder="Type your message..."></textarea>
                <button class="send-button">Send</button>
            </div>
        `;
        
        tabContent.appendChild(chatPanel);
    }
    
    // Get agent status
    function getAgentStatus() {
        fetch('/api/status')
        .then(response => response.json())
        .then(data => {
            if (data.status === 'success') {
                updateAgentStatus(data.agents);
            }
        })
        .catch(error => {
            console.error('Error getting agent status:', error);
        });
    }
    
    // Update agent status in UI
    function updateAgentStatus(agents) {
        statusList.innerHTML = '';
        
        for (const [agent, status] of Object.entries(agents)) {
            const listItem = document.createElement('li');
            
            const nameSpan = document.createElement('span');
            nameSpan.textContent = agent.charAt(0).toUpperCase() + agent.slice(1);
            
            const statusSpan = document.createElement('span');
            statusSpan.textContent = status;
            statusSpan.classList.add(status === 'running' ? 'status-running' : 'status-idle');
            
            listItem.appendChild(nameSpan);
            listItem.appendChild(statusSpan);
            
            statusList.appendChild(listItem);
            
            // Add tab for agent if it doesn't exist
            if (status === 'running' && !document.querySelector(`.tab-button[data-agent="${agent}"]`)) {
                addAgentTab(agent);
            }
        }
    }
    
    // Initialize WebSocket
    initWebSocket();
    
    // Initial agent status check
    getAgentStatus();
});
