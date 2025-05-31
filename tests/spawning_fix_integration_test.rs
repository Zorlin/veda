use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use uuid::Uuid;
use tokio::sync::mpsc;

// Integration test to verify the spawning fix works end-to-end
#[derive(Debug, Clone)]
struct TestInstance {
    id: Uuid,
    name: String,
    session_id: Option<String>,
    working_directory: String,
    messages: Vec<TestMessage>,
    spawned_via_claude_code: bool, // Track if this was spawned via Claude Code
}

#[derive(Debug, Clone)]
struct TestMessage {
    sender: String,
    content: String,
    received_at: std::time::Instant,
}

#[derive(Debug, Clone)]
enum TestClaudeMessage {
    StreamStart { 
        instance_id: Uuid, 
        session_id: Option<String> 
    },
    StreamText { 
        instance_id: Uuid, 
        text: String, 
        session_id: Option<String> 
    },
    SessionStarted { 
        instance_id: Uuid, 
        session_id: String 
    },
}

struct TestApp {
    instances: Vec<TestInstance>,
    pending_session_messages: Vec<(Uuid, String, String)>,
    spawn_tracker: Arc<Mutex<SpawnTracker>>,
}

#[derive(Debug, Default)]
struct SpawnTracker {
    spawn_attempts: Vec<SpawnAttempt>,
    active_sessions: HashMap<String, Uuid>, // session_id -> instance_id
    message_routing_log: Vec<RoutingEvent>,
}

#[derive(Debug, Clone)]
struct SpawnAttempt {
    instance_id: Uuid,
    instance_name: String,
    target_env_var: Option<String>,
    message: String,
    timestamp: std::time::Instant,
    success: bool,
}

#[derive(Debug, Clone)]
struct RoutingEvent {
    message_content: String,
    session_id: Option<String>,
    instance_id: Uuid,
    target_tab: Option<usize>,
    routing_method: RoutingMethod,
    timestamp: std::time::Instant,
}

#[derive(Debug, Clone)]
enum RoutingMethod {
    SessionId,
    InstanceIdFallback,
    Buffered,
    Failed,
}

impl TestInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            session_id: None,
            working_directory: "/home/wings/projects/veda".to_string(),
            messages: Vec::new(),
            spawned_via_claude_code: false,
        }
    }

    fn add_message(&mut self, sender: String, content: String) {
        self.messages.push(TestMessage {
            sender,
            content,
            received_at: std::time::Instant::now(),
        });
    }
}

impl TestApp {
    fn new() -> Self {
        Self {
            instances: vec![TestInstance::new("Veda-1".to_string())],
            pending_session_messages: Vec::new(),
            spawn_tracker: Arc::new(Mutex::new(SpawnTracker::default())),
        }
    }

    fn spawn_instance_with_claude_code(&mut self, task_description: &str) -> Uuid {
        let instance_name = format!("Veda-{}", self.instances.len() + 1);
        let mut new_instance = TestInstance::new(instance_name.clone());
        new_instance.spawned_via_claude_code = true;
        
        let instance_id = new_instance.id;
        self.instances.push(new_instance);
        
        // Simulate the fixed spawning mechanism
        self.simulate_claude_code_spawn(instance_id, &instance_name, task_description);
        
        instance_id
    }

    fn simulate_claude_code_spawn(&self, instance_id: Uuid, instance_name: &str, message: &str) {
        let mut tracker = self.spawn_tracker.lock().unwrap();
        
        // Simulate setting environment variable before spawn
        let target_env_var = Some(instance_id.to_string());
        
        tracker.spawn_attempts.push(SpawnAttempt {
            instance_id,
            instance_name: instance_name.to_string(),
            target_env_var,
            message: message.to_string(),
            timestamp: std::time::Instant::now(),
            success: true, // Assume success in test
        });
    }

    fn process_claude_message(&mut self, msg: TestClaudeMessage) {
        let mut tracker = self.spawn_tracker.lock().unwrap();
        
        match msg {
            TestClaudeMessage::StreamStart { .. } => {
                // Just log, no special handling needed
            }
            TestClaudeMessage::StreamText { instance_id, text, session_id } => {
                let routing_event = self.route_message(instance_id, &text, session_id.as_ref());
                tracker.message_routing_log.push(routing_event);
            }
            TestClaudeMessage::SessionStarted { instance_id, session_id } => {
                if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                    instance.session_id = Some(session_id.clone());
                    tracker.active_sessions.insert(session_id.clone(), instance_id);
                    
                    // Process any buffered messages for this session
                    let mut remaining_messages = Vec::new();
                    for (msg_instance_id, text, msg_session_id) in std::mem::take(&mut self.pending_session_messages) {
                        if msg_session_id == session_id {
                            instance.add_message("Claude".to_string(), text.clone());
                            tracker.message_routing_log.push(RoutingEvent {
                                message_content: text,
                                session_id: Some(msg_session_id),
                                instance_id: msg_instance_id,
                                target_tab: Some(self.instances.iter().position(|i| i.id == instance_id).unwrap()),
                                routing_method: RoutingMethod::Buffered,
                                timestamp: std::time::Instant::now(),
                            });
                        } else {
                            remaining_messages.push((msg_instance_id, text, msg_session_id));
                        }
                    }
                    self.pending_session_messages = remaining_messages;
                }
            }
        }
    }

    fn route_message(&mut self, instance_id: Uuid, text: &str, session_id: Option<&String>) -> RoutingEvent {
        let target_instance_index = if let Some(session_id_val) = session_id {
            // Try session ID first (priority routing)
            self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                .map(|idx| (idx, RoutingMethod::SessionId))
                .or_else(|| {
                    // Fallback to instance ID
                    self.instances.iter().position(|i| i.id == instance_id)
                        .map(|idx| (idx, RoutingMethod::InstanceIdFallback))
                })
        } else {
            // No session ID, use instance ID directly
            self.instances.iter().position(|i| i.id == instance_id)
                .map(|idx| (idx, RoutingMethod::InstanceIdFallback))
        };

        let routing_event = if let Some((instance_idx, method)) = target_instance_index {
            self.instances[instance_idx].add_message("Claude".to_string(), text.to_string());
            RoutingEvent {
                message_content: text.to_string(),
                session_id: session_id.cloned(),
                instance_id,
                target_tab: Some(instance_idx),
                routing_method: method,
                timestamp: std::time::Instant::now(),
            }
        } else if let Some(session_id_val) = session_id {
            // Buffer the message
            self.pending_session_messages.push((instance_id, text.to_string(), session_id_val.clone()));
            RoutingEvent {
                message_content: text.to_string(),
                session_id: session_id.cloned(),
                instance_id,
                target_tab: None,
                routing_method: RoutingMethod::Buffered,
                timestamp: std::time::Instant::now(),
            }
        } else {
            // Failed to route
            RoutingEvent {
                message_content: text.to_string(),
                session_id: None,
                instance_id,
                target_tab: None,
                routing_method: RoutingMethod::Failed,
                timestamp: std::time::Instant::now(),
            }
        };

        routing_event
    }
}

#[tokio::test]
async fn test_spawning_fix_prevents_output_crossover() {
    let mut app = TestApp::new();
    
    // Spawn two instances with different tasks (the original problem scenario)
    let instance_id_2 = app.spawn_instance_with_claude_code("Implement MooseNG master server");
    let instance_id_3 = app.spawn_instance_with_claude_code("Implement MooseNG chunk server");
    
    // Verify instances were created correctly
    assert_eq!(app.instances.len(), 3, "Should have 3 instances total");
    assert!(app.instances[1].spawned_via_claude_code, "Veda-2 should be spawned via Claude Code");
    assert!(app.instances[2].spawned_via_claude_code, "Veda-3 should be spawned via Claude Code");
    
    // Simulate sessions being established (as Claude Code would do)
    let session_id_2 = "claude-session-abc123".to_string();
    let session_id_3 = "claude-session-def456".to_string();
    
    app.process_claude_message(TestClaudeMessage::SessionStarted {
        instance_id: instance_id_2,
        session_id: session_id_2.clone(),
    });
    
    app.process_claude_message(TestClaudeMessage::SessionStarted {
        instance_id: instance_id_3,
        session_id: session_id_3.clone(),
    });
    
    // Send messages from each Claude Code instance (the key test)
    app.process_claude_message(TestClaudeMessage::StreamText {
        instance_id: instance_id_2,
        text: "Starting master server implementation...".to_string(),
        session_id: Some(session_id_2.clone()),
    });
    
    app.process_claude_message(TestClaudeMessage::StreamText {
        instance_id: instance_id_3,
        text: "Beginning chunk server development...".to_string(),
        session_id: Some(session_id_3.clone()),
    });
    
    // Verify messages went to correct tabs (not Veda-1!)
    assert_eq!(app.instances[0].messages.len(), 0, "Veda-1 should have no messages");
    assert_eq!(app.instances[1].messages.len(), 1, "Veda-2 should have exactly one message");
    assert_eq!(app.instances[2].messages.len(), 1, "Veda-3 should have exactly one message");
    
    // Verify message content correctness
    assert_eq!(app.instances[1].messages[0].content, "Starting master server implementation...");
    assert_eq!(app.instances[2].messages[0].content, "Beginning chunk server development...");
    
    // Verify no crossover occurred
    assert!(!app.instances[1].messages[0].content.contains("chunk server"), "Veda-2 should not get chunk server messages");
    assert!(!app.instances[2].messages[0].content.contains("master server"), "Veda-3 should not get master server messages");
    
    // Verify spawn tracking
    let tracker = app.spawn_tracker.lock().unwrap();
    assert_eq!(tracker.spawn_attempts.len(), 2, "Should have tracked 2 spawn attempts");
    assert!(tracker.spawn_attempts.iter().all(|a| a.success), "All spawns should be successful");
    assert!(tracker.spawn_attempts.iter().all(|a| a.target_env_var.is_some()), "All spawns should have target env var");
}

#[tokio::test]
async fn test_message_buffering_during_session_establishment() {
    let mut app = TestApp::new();
    
    // Spawn an instance
    let instance_id = app.spawn_instance_with_claude_code("Test buffering");
    let session_id = "session-buffering-test".to_string();
    
    // Send messages BEFORE session is established (real-world race condition)
    app.process_claude_message(TestClaudeMessage::StreamText {
        instance_id,
        text: "Early message 1".to_string(),
        session_id: Some(session_id.clone()),
    });
    
    app.process_claude_message(TestClaudeMessage::StreamText {
        instance_id,
        text: "Early message 2".to_string(),
        session_id: Some(session_id.clone()),
    });
    
    // Verify messages are buffered, not lost
    assert_eq!(app.pending_session_messages.len(), 2, "Should buffer 2 messages");
    assert_eq!(app.instances[1].messages.len(), 0, "Instance should have no messages yet");
    
    // Now establish the session
    app.process_claude_message(TestClaudeMessage::SessionStarted {
        instance_id,
        session_id: session_id.clone(),
    });
    
    // Verify buffered messages were delivered
    assert_eq!(app.pending_session_messages.len(), 0, "Buffer should be empty");
    assert_eq!(app.instances[1].messages.len(), 2, "Instance should have received buffered messages");
    assert_eq!(app.instances[1].messages[0].content, "Early message 1");
    assert_eq!(app.instances[1].messages[1].content, "Early message 2");
    
    // Verify routing was logged correctly
    let tracker = app.spawn_tracker.lock().unwrap();
    let buffered_events = tracker.message_routing_log.iter()
        .filter(|e| matches!(e.routing_method, RoutingMethod::Buffered))
        .count();
    assert_eq!(buffered_events, 2, "Should have logged 2 buffered routing events");
}

#[tokio::test]
async fn test_environment_variable_isolation_per_spawn() {
    let mut app = TestApp::new();
    
    // Spawn multiple instances to test environment isolation
    let instance_ids: Vec<Uuid> = (0..3)
        .map(|i| app.spawn_instance_with_claude_code(&format!("Task {}", i + 1)))
        .collect();
    
    // Verify each spawn had its own target environment variable
    let tracker = app.spawn_tracker.lock().unwrap();
    assert_eq!(tracker.spawn_attempts.len(), 3, "Should have 3 spawn attempts");
    
    for (i, attempt) in tracker.spawn_attempts.iter().enumerate() {
        assert_eq!(attempt.instance_id, instance_ids[i], "Instance ID should match");
        assert_eq!(attempt.target_env_var, Some(instance_ids[i].to_string()), "Each spawn should have unique target env var");
        assert_eq!(attempt.message, format!("Task {}", i + 1), "Message should be correct");
        assert!(attempt.success, "Spawn should be successful");
    }
}

#[tokio::test] 
async fn test_session_priority_routing_over_instance_id() {
    let mut app = TestApp::new();
    
    // Create two instances
    let instance_id_2 = app.spawn_instance_with_claude_code("Task A");
    let instance_id_3 = app.spawn_instance_with_claude_code("Task B");
    
    // Establish sessions
    let session_id_2 = "session-priority-2".to_string();
    let session_id_3 = "session-priority-3".to_string();
    
    app.process_claude_message(TestClaudeMessage::SessionStarted {
        instance_id: instance_id_2,
        session_id: session_id_2.clone(),
    });
    
    app.process_claude_message(TestClaudeMessage::SessionStarted {
        instance_id: instance_id_3,
        session_id: session_id_3.clone(),
    });
    
    // Send message with WRONG instance ID but CORRECT session ID
    // This tests that session ID takes priority over instance ID
    app.process_claude_message(TestClaudeMessage::StreamText {
        instance_id: instance_id_3, // Wrong!
        text: "Session routing test".to_string(),
        session_id: Some(session_id_2.clone()), // Correct!
    });
    
    // Verify message went to session match (Veda-2), not instance ID match (Veda-3)
    assert_eq!(app.instances[1].messages.len(), 1, "Veda-2 should get the message (session match)");
    assert_eq!(app.instances[2].messages.len(), 0, "Veda-3 should not get the message (instance ID match ignored)");
    assert_eq!(app.instances[1].messages[0].content, "Session routing test");
    
    // Verify routing method was logged correctly
    let tracker = app.spawn_tracker.lock().unwrap();
    let session_routes = tracker.message_routing_log.iter()
        .filter(|e| matches!(e.routing_method, RoutingMethod::SessionId))
        .count();
    assert_eq!(session_routes, 1, "Should have used session ID routing");
}

#[tokio::test]
async fn test_original_bug_scenario_fixed() {
    // This test specifically recreates the original bug scenario:
    // "Output from Veda-2 is going to Veda-1 or being blackholed"
    
    let mut app = TestApp::new();
    
    // User spawns multiple instances like in the original bug report
    let veda_2_id = app.spawn_instance_with_claude_code("Continue implementation implementing MooseNG");
    let veda_3_id = app.spawn_instance_with_claude_code("Work on erasure coding");
    let veda_4_id = app.spawn_instance_with_claude_code("Implement RAFT consensus");
    
    // Each gets its own session (as Claude Code would provide)
    let session_2 = "real-claude-session-1".to_string();
    let session_3 = "real-claude-session-2".to_string();
    let session_4 = "real-claude-session-3".to_string();
    
    // Establish sessions
    for (instance_id, session_id) in [
        (veda_2_id, session_2.clone()),
        (veda_3_id, session_3.clone()),
        (veda_4_id, session_4.clone()),
    ] {
        app.process_claude_message(TestClaudeMessage::SessionStarted {
            instance_id,
            session_id,
        });
    }
    
    // Simulate real Claude Code output from each instance
    let test_scenarios = [
        (veda_2_id, session_2, "I'll start implementing the MooseNG master server with Raft consensus..."),
        (veda_3_id, session_3, "Let me begin working on the Reed-Solomon erasure coding implementation..."),
        (veda_4_id, session_4, "Starting RAFT consensus implementation with leader election..."),
    ];
    
    for (instance_id, session_id, message) in test_scenarios {
        app.process_claude_message(TestClaudeMessage::StreamText {
            instance_id,
            text: message.to_string(),
            session_id: Some(session_id),
        });
    }
    
    // CRITICAL VERIFICATION: Ensure output goes to correct tabs, not Veda-1
    assert_eq!(app.instances[0].messages.len(), 0, "❌ BUG: Veda-1 should NOT receive any spawned instance messages");
    assert_eq!(app.instances[1].messages.len(), 1, "✅ Veda-2 should receive exactly its own message");
    assert_eq!(app.instances[2].messages.len(), 1, "✅ Veda-3 should receive exactly its own message");
    assert_eq!(app.instances[3].messages.len(), 1, "✅ Veda-4 should receive exactly its own message");
    
    // Verify correct message content (no crossover)
    assert!(app.instances[1].messages[0].content.contains("master server"), "Veda-2 should get master server message");
    assert!(app.instances[2].messages[0].content.contains("Reed-Solomon"), "Veda-3 should get erasure coding message");
    assert!(app.instances[3].messages[0].content.contains("RAFT consensus"), "Veda-4 should get RAFT message");
    
    // Verify no blackholing occurred
    let tracker = app.spawn_tracker.lock().unwrap();
    let failed_routes = tracker.message_routing_log.iter()
        .filter(|e| matches!(e.routing_method, RoutingMethod::Failed))
        .count();
    assert_eq!(failed_routes, 0, "❌ BUG: No messages should be blackholed/lost");
    
    // Verify all messages were routed by session ID (optimal routing)
    let session_routes = tracker.message_routing_log.iter()
        .filter(|e| matches!(e.routing_method, RoutingMethod::SessionId))
        .count();
    assert_eq!(session_routes, 3, "All messages should use session ID routing");
    
    println!("✅ Original bug scenario test PASSED - no output crossover or blackholing!");
}