# Veda Cross-Codebase Coordination System

## Overview

The Cross-Codebase Coordination System transforms multiple Veda instances into a swarm of intelligent agents, each responsible for their own codebase, communicating through a unified message bus to coordinate changes, negotiate interfaces, and synchronize development across repository boundaries.

## Architecture

### Core Components

```
┌─────────────────────────────────────────────────────────────────┐
│                    Veda Coordination Bus                         │
│                 Unix Domain Socket: /tmp/veda-bus.sock          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐│
│  │ Flagship │    │ Lens SDK │    │Auth Core │    │  UI Lib  ││
│  │  Veda    │◄──►│   Veda   │◄──►│  Veda    │◄──►│  Veda    ││
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘│
│       ▲                ▲                ▲                ▲      │
│       │                │                │                │      │
│       ▼                ▼                ▼                ▼      │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │              Message Router & Registry                    │ │
│  │  • Route messages by target_repo                         │ │
│  │  • Track active instances                                │ │
│  │  • Buffer messages for offline instances                 │ │
│  │  • Log coordination history                              │ │
│  └──────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### Message Protocol

#### Base Message Structure
```json
{
  "id": "uuid-v4",
  "from": {
    "repo": "Flagship",
    "path": "/home/user/projects/flagship",
    "session_id": "session-uuid",
    "instance_id": "instance-uuid"
  },
  "to": {
    "repo": "LensSDK",  // or "*" for broadcast
    "capabilities": ["auth", "session"]  // optional capability filter
  },
  "type": "RequestChange|Query|Notify|Negotiate|Task",
  "priority": "high|normal|low",
  "payload": { /* type-specific data */ },
  "timestamp": 1717689033,
  "reply_to": "parent-message-id",  // for threading
  "ttl": 300  // seconds before message expires
}
```

#### Message Types

##### 1. RequestChange
```json
{
  "type": "RequestChange",
  "payload": {
    "summary": "Expose getActiveSession() in session.rs",
    "reason": "Used in Flagship for auto-resume on reconnect",
    "suggested_implementation": "pub fn getActiveSession() -> Option<Session>",
    "urgency": "blocking_development"
  }
}
```

##### 2. Query
```json
{
  "type": "Query",
  "payload": {
    "question": "What's the current interface for SessionManager?",
    "context": "Planning to integrate with new auth flow",
    "response_format": "rust_trait_definition"
  }
}
```

##### 3. Notify
```json
{
  "type": "Notify",
  "payload": {
    "change_type": "breaking_change|deprecation|new_feature",
    "summary": "Renamed SessionContext to ActiveSessionContext",
    "migration_guide": "sed -i 's/SessionContext/ActiveSessionContext/g'",
    "effective_version": "2.0.0"
  }
}
```

##### 4. Negotiate
```json
{
  "type": "Negotiate",
  "payload": {
    "proposal": "Move shared auth logic to auth-core package",
    "affected_repos": ["Flagship", "LensSDK"],
    "benefits": ["Reduced duplication", "Centralized security updates"],
    "requires_consensus": true
  }
}
```

##### 5. Task
```json
{
  "type": "Task",
  "payload": {
    "task_type": "distributed_refactor|test_suite|benchmark",
    "description": "Update all repos to use new ErrorBoundary API",
    "subtasks": {
      "Flagship": "Update 12 components",
      "UILib": "Export new ErrorBoundary",
      "LensSDK": "Add error context provider"
    }
  }
}
```

### Veda Agent Extensions

Each Veda instance gains new capabilities:

#### 1. Coordination Handler
```rust
// In each Veda instance
struct CoordinationHandler {
    repo_name: String,
    repo_path: PathBuf,
    bus_client: BusClient,
    message_buffer: VecDeque<CoordinationMessage>,
}

impl CoordinationHandler {
    async fn handle_incoming(&mut self, msg: CoordinationMessage) {
        match msg.message_type {
            MessageType::RequestChange => self.handle_change_request(msg).await,
            MessageType::Query => self.handle_query(msg).await,
            MessageType::Notify => self.handle_notification(msg).await,
            MessageType::Negotiate => self.handle_negotiation(msg).await,
            MessageType::Task => self.handle_task_assignment(msg).await,
        }
    }
}
```

#### 2. Claude Integration

New system prompts for Claude when coordination is active:

```
You are managing the {repo_name} codebase as part of a distributed development swarm.

COORDINATION CONTEXT:
- Other active repositories: {active_repos}
- Your capabilities: {declared_capabilities}
- Pending requests: {pending_requests}

When you receive coordination messages:
1. Acknowledge receipt immediately
2. Assess feasibility and impact
3. Create tasks in TaskMaster for implementation
4. Respond with timeline or concerns
5. Coordinate with other repos if changes cascade

You can send coordination messages using:
- send_coordination_message(to, type, payload)
- broadcast_coordination(type, payload)
- query_codebase(repo, question)
```

### Implementation Components

#### 1. Message Bus Daemon (`veda-bus`)
```rust
// Standalone daemon managing the coordination bus
#[tokio::main]
async fn main() {
    let bus = CoordinationBus::new("/tmp/veda-bus.sock");
    
    // Track connected instances
    let registry = InstanceRegistry::new();
    
    // Message routing
    bus.on_message(|msg, registry| {
        if msg.to.repo == "*" {
            registry.broadcast(msg);
        } else {
            registry.route_to(msg.to.repo, msg);
        }
    });
    
    bus.serve().await;
}
```

#### 2. Veda Bus Client
```rust
// Added to each Veda instance
impl App {
    async fn connect_to_coordination_bus(&mut self) -> Result<()> {
        let client = BusClient::connect("/tmp/veda-bus.sock").await?;
        
        // Register this instance
        client.register(RegisterMessage {
            repo_name: self.repo_name.clone(),
            repo_path: self.working_directory.clone(),
            capabilities: self.detect_capabilities(),
            session_id: self.session_id.clone(),
        }).await?;
        
        // Start listening for messages
        let handler = self.coordination_handler.clone();
        tokio::spawn(async move {
            while let Some(msg) = client.receive().await {
                handler.handle_incoming(msg).await;
            }
        });
        
        Ok(())
    }
}
```

#### 3. MCP Tools for Cross-Repo Operations

New MCP tools exposed to Claude:

```json
{
  "name": "send_coordination_message",
  "description": "Send a message to another codebase's Veda instance",
  "parameters": {
    "to_repo": "Target repository name",
    "message_type": "RequestChange|Query|Notify|Negotiate|Task",
    "payload": "Message-specific payload object"
  }
}

{
  "name": "query_codebase",
  "description": "Ask a question about another codebase",
  "parameters": {
    "repo": "Repository to query",
    "question": "Natural language question",
    "context": "Additional context for the query"
  }
}

{
  "name": "broadcast_coordination",
  "description": "Broadcast a message to all connected Veda instances",
  "parameters": {
    "message_type": "Notify|Task",
    "payload": "Broadcast payload"
  }
}
```

## Use Cases

### 1. API Contract Negotiation
```
Flagship → LensSDK: "Need getActiveSession() - returns current user session"
LensSDK → Flagship: "Can provide, but returns Result<Session, AuthError> for safety"
Flagship → LensSDK: "Accepted. When available?"
LensSDK → Flagship: "Implementing now. ETA 5 minutes"
```

### 2. Cascading Refactor
```
AuthCore → *: "Moving to async auth. All authenticate() calls must be awaited"
Flagship → AuthCore: "Acknowledged. Found 23 call sites. Beginning migration"
LensSDK → AuthCore: "Acknowledged. Found 45 call sites. Need migration guide"
AuthCore → LensSDK: "Guide: {automated sed script + manual review points}"
```

### 3. Cross-Repo Testing
```
UILib → *: "Released ErrorBoundary 2.0. Please test integration"
Flagship → UILib: "Running integration tests..."
Flagship → UILib: "FAIL: Missing onReset callback in props"
UILib → Flagship: "Fixed in 2.0.1. Please retest"
```

### 4. Distributed Task Execution
```
Orchestrator → *: "Task: Implement new logging standard RFC-123"
Flagship → Orchestrator: "Assigned subtask: Update 45 log statements"
LensSDK → Orchestrator: "Assigned subtask: Add structured logging to API layer"
AuthCore → Orchestrator: "Assigned subtask: Implement log aggregation client"
```

## Security & Isolation

1. **Process Isolation**: Each Veda runs in its own process with its own Claude instances
2. **Message Validation**: All messages are validated against schema before routing
3. **Capability-Based Access**: Repos declare and check capabilities before accepting requests
4. **Audit Trail**: All coordination messages are logged for review
5. **TTL Enforcement**: Messages expire to prevent stale operations

## Configuration

### Per-Repository Config (`.veda/coordination.toml`)
```toml
[coordination]
repo_name = "Flagship"
capabilities = ["ui", "auth", "api_client"]
accept_requests_from = ["*"]  # or ["LensSDK", "AuthCore"]
auto_acknowledge = true
max_message_age = 300  # seconds

[coordination.filters]
# Only show high-priority messages to Claude
claude_message_filter = "priority >= normal"
```

### Global Config (`~/.veda/bus.toml`)
```toml
[bus]
socket_path = "/tmp/veda-bus.sock"
max_message_size = 1048576  # 1MB
message_retention = 3600  # 1 hour
enable_logging = true
log_path = "~/.veda/coordination.log"
```

## Benefits

1. **True Multi-Repo Development**: Changes cascade intelligently across boundaries
2. **Reduced Integration Failures**: APIs are negotiated, not assumed
3. **Distributed Intelligence**: Each repo has its own Claude expert
4. **Automatic Dependency Management**: Changes propagate with context
5. **Swarm Problem Solving**: Complex tasks distributed across specialized agents

## Future Enhancements

1. **Remote Coordination**: TCP sockets for distributed teams
2. **Conflict Resolution**: Automated negotiation for competing changes
3. **Dependency Graphs**: Visual representation of cross-repo dependencies
4. **Change Impact Analysis**: Predict ripple effects before implementation
5. **Automated Integration Testing**: Trigger cross-repo test suites on changes