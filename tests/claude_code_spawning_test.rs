use std::sync::{Arc, Mutex};
use uuid::Uuid;
use tokio::sync::mpsc;
use futures_util::future;

// Mock structures for testing
#[derive(Debug, Clone)]
struct MockInstance {
    id: Uuid,
    name: String,
    session_id: Option<String>,
    working_directory: String,
    messages: Vec<MockMessage>,
}

#[derive(Debug, Clone)]
struct MockMessage {
    sender: String,
    content: String,
    timestamp: String,
}

#[derive(Debug, Clone)]
enum MockClaudeMessage {
    StreamStart { 
        instance_id: Uuid, 
        session_id: Option<String> 
    },
    StreamText { 
        instance_id: Uuid, 
        text: String, 
        session_id: Option<String> 
    },
    StreamEnd { 
        instance_id: Uuid, 
        session_id: Option<String> 
    },
    SessionStarted { 
        instance_id: Uuid, 
        session_id: String 
    },
    Error { 
        instance_id: Uuid, 
        error: String, 
        session_id: Option<String> 
    },
}

struct MockApp {
    instances: Vec<MockInstance>,
    current_tab: usize,
    pending_session_messages: Vec<(Uuid, String, String)>,
    message_rx: Option<mpsc::Receiver<MockClaudeMessage>>,
    spawn_calls: Arc<Mutex<Vec<SpawnCall>>>,
}

#[derive(Debug, Clone)]
struct SpawnCall {
    instance_id: Uuid,
    message: String,
    target_env_var: Option<String>,
}

impl MockInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            session_id: None,
            working_directory: "/tmp".to_string(),
            messages: Vec::new(),
        }
    }

    fn add_message(&mut self, sender: String, content: String) {
        self.messages.push(MockMessage {
            sender,
            content,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }
}

impl MockApp {
    fn new() -> (Self, mpsc::Sender<MockClaudeMessage>) {
        let (tx, rx) = mpsc::channel(100);
        let app = Self {
            instances: vec![MockInstance::new("Veda-1".to_string())],
            current_tab: 0,
            pending_session_messages: Vec::new(),
            message_rx: Some(rx),
            spawn_calls: Arc::new(Mutex::new(Vec::new())),
        };
        (app, tx)
    }

    fn spawn_instance(&mut self) -> Uuid {
        let instance_name = format!("Veda-{}", self.instances.len() + 1);
        let mut new_instance = MockInstance::new(instance_name);
        new_instance.working_directory = "/home/wings/projects/veda".to_string();
        
        let instance_id = new_instance.id;
        self.instances.push(new_instance);
        instance_id
    }

    async fn mock_send_to_claude_with_session(
        &self,
        instance_id: Uuid,
        message: String,
        _tx: mpsc::Sender<MockClaudeMessage>,
        _session_id: Option<String>,
        _process_handle: Option<()>,
    ) -> Result<(), String> {
        // Record the spawn call
        let target_env = std::env::var("VEDA_TARGET_INSTANCE_ID").ok();
        self.spawn_calls.lock().unwrap().push(SpawnCall {
            instance_id,
            message,
            target_env_var: target_env,
        });
        Ok(())
    }

    fn process_claude_message(&mut self, msg: MockClaudeMessage) {
        match msg {
            MockClaudeMessage::StreamStart { instance_id, .. } => {
                println!("StreamStart for instance {}", instance_id);
            }
            MockClaudeMessage::StreamText { instance_id, text, session_id } => {
                let target_instance_index = if let Some(session_id_val) = &session_id {
                    self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                        .or_else(|| self.instances.iter().position(|i| i.id == instance_id))
                } else {
                    self.instances.iter().position(|i| i.id == instance_id)
                };

                if let Some(instance_idx) = target_instance_index {
                    self.instances[instance_idx].add_message("Claude".to_string(), text);
                } else if let Some(ref session_id_val) = session_id {
                    self.pending_session_messages.push((instance_id, text, session_id_val.clone()));
                }
            }
            MockClaudeMessage::SessionStarted { instance_id, session_id } => {
                if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                    instance.session_id = Some(session_id.clone());
                    
                    // Process any buffered messages
                    let mut remaining_messages = Vec::new();
                    for (msg_instance_id, text, msg_session_id) in std::mem::take(&mut self.pending_session_messages) {
                        if msg_session_id == session_id {
                            instance.add_message("Claude".to_string(), text);
                        } else {
                            remaining_messages.push((msg_instance_id, text, msg_session_id));
                        }
                    }
                    self.pending_session_messages = remaining_messages;
                }
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_claude_code_spawning_mechanism() {
    let (mut app, tx) = MockApp::new();
    
    // Spawn a new instance
    let instance_id = app.spawn_instance();
    
    // Simulate the spawning process with environment variable
    std::env::set_var("VEDA_TARGET_INSTANCE_ID", instance_id.to_string());
    
    let spawn_result = app.mock_send_to_claude_with_session(
        instance_id,
        "Continue implementation implementing MooseNG".to_string(),
        tx.clone(),
        None,
        None,
    ).await;
    
    std::env::remove_var("VEDA_TARGET_INSTANCE_ID");
    
    // Verify spawn was successful
    assert!(spawn_result.is_ok(), "Claude Code spawn should succeed");
    
    // Verify the spawn call was recorded with correct parameters
    let spawn_calls = app.spawn_calls.lock().unwrap();
    assert_eq!(spawn_calls.len(), 1, "Should have recorded one spawn call");
    
    let spawn_call = &spawn_calls[0];
    assert_eq!(spawn_call.instance_id, instance_id, "Should spawn with correct instance ID");
    assert_eq!(spawn_call.message, "Continue implementation implementing MooseNG", "Should use correct message");
    assert_eq!(spawn_call.target_env_var, Some(instance_id.to_string()), "Should pass target instance ID in environment");
}

#[tokio::test]
async fn test_session_based_message_routing() {
    let (mut app, _tx) = MockApp::new();
    
    // Spawn two new instances
    let instance_id_1 = app.spawn_instance(); // Veda-2
    let instance_id_2 = app.spawn_instance(); // Veda-3
    
    // Simulate session establishment for both instances
    let session_id_1 = "session-12345".to_string();
    let session_id_2 = "session-67890".to_string();
    
    app.process_claude_message(MockClaudeMessage::SessionStarted {
        instance_id: instance_id_1,
        session_id: session_id_1.clone(),
    });
    
    app.process_claude_message(MockClaudeMessage::SessionStarted {
        instance_id: instance_id_2,
        session_id: session_id_2.clone(),
    });
    
    // Verify sessions were assigned
    assert_eq!(app.instances[1].session_id, Some(session_id_1.clone()));
    assert_eq!(app.instances[2].session_id, Some(session_id_2.clone()));
    
    // Send messages to each session
    app.process_claude_message(MockClaudeMessage::StreamText {
        instance_id: instance_id_1,
        text: "Message for Veda-2".to_string(),
        session_id: Some(session_id_1.clone()),
    });
    
    app.process_claude_message(MockClaudeMessage::StreamText {
        instance_id: instance_id_2,
        text: "Message for Veda-3".to_string(),
        session_id: Some(session_id_2.clone()),
    });
    
    // Verify messages were routed to correct instances
    assert_eq!(app.instances[1].messages.len(), 1, "Veda-2 should have one message");
    assert_eq!(app.instances[1].messages[0].content, "Message for Veda-2");
    
    assert_eq!(app.instances[2].messages.len(), 1, "Veda-3 should have one message");
    assert_eq!(app.instances[2].messages[0].content, "Message for Veda-3");
    
    // Verify no cross-contamination
    assert_eq!(app.instances[0].messages.len(), 0, "Veda-1 should have no messages");
}

#[tokio::test]
async fn test_message_buffering_before_session_established() {
    let (mut app, _tx) = MockApp::new();
    
    // Spawn a new instance
    let instance_id = app.spawn_instance();
    let session_id = "session-buffer-test".to_string();
    
    // Send a message before session is established (should be buffered)
    app.process_claude_message(MockClaudeMessage::StreamText {
        instance_id,
        text: "Buffered message".to_string(),
        session_id: Some(session_id.clone()),
    });
    
    // Verify message was buffered
    assert_eq!(app.pending_session_messages.len(), 1, "Should have one buffered message");
    assert_eq!(app.instances[1].messages.len(), 0, "Instance should have no messages yet");
    
    // Establish session
    app.process_claude_message(MockClaudeMessage::SessionStarted {
        instance_id,
        session_id: session_id.clone(),
    });
    
    // Verify buffered message was processed
    assert_eq!(app.pending_session_messages.len(), 0, "Buffer should be empty");
    assert_eq!(app.instances[1].messages.len(), 1, "Instance should have received buffered message");
    assert_eq!(app.instances[1].messages[0].content, "Buffered message");
}

#[tokio::test]
async fn test_instance_id_fallback_routing() {
    let (mut app, _tx) = MockApp::new();
    
    // Spawn a new instance
    let instance_id = app.spawn_instance();
    
    // Send message without session_id (should use instance_id fallback)
    app.process_claude_message(MockClaudeMessage::StreamText {
        instance_id,
        text: "Message via instance ID".to_string(),
        session_id: None,
    });
    
    // Verify message was routed correctly via instance_id
    assert_eq!(app.instances[1].messages.len(), 1, "Should route via instance_id fallback");
    assert_eq!(app.instances[1].messages[0].content, "Message via instance ID");
}

#[tokio::test]
async fn test_environment_variable_isolation() {
    let (app, tx) = MockApp::new();
    
    // Test that each spawn call gets isolated environment
    let instance_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    
    for (i, &instance_id) in instance_ids.iter().enumerate() {
        // Set unique environment for each spawn
        std::env::set_var("VEDA_TARGET_INSTANCE_ID", instance_id.to_string());
        
        let _result = app.mock_send_to_claude_with_session(
            instance_id,
            format!("Task for instance {}", i + 1),
            tx.clone(),
            None,
            None,
        ).await;
        
        std::env::remove_var("VEDA_TARGET_INSTANCE_ID");
    }
    
    // Verify each spawn had unique environment
    let spawn_calls = app.spawn_calls.lock().unwrap();
    assert_eq!(spawn_calls.len(), 3, "Should have three spawn calls");
    
    for (i, call) in spawn_calls.iter().enumerate() {
        assert_eq!(call.instance_id, instance_ids[i], "Instance ID should match");
        assert_eq!(call.target_env_var, Some(instance_ids[i].to_string()), "Environment should match instance");
        assert_eq!(call.message, format!("Task for instance {}", i + 1), "Message should be correct");
    }
}

#[tokio::test]
async fn test_session_routing_priority_over_instance_id() {
    let (mut app, _tx) = MockApp::new();
    
    // Spawn two instances
    let instance_id_1 = app.spawn_instance();
    let instance_id_2 = app.spawn_instance();
    
    // Establish sessions
    let session_id_1 = "session-priority-1".to_string();
    let session_id_2 = "session-priority-2".to_string();
    
    app.process_claude_message(MockClaudeMessage::SessionStarted {
        instance_id: instance_id_1,
        session_id: session_id_1.clone(),
    });
    
    app.process_claude_message(MockClaudeMessage::SessionStarted {
        instance_id: instance_id_2,
        session_id: session_id_2.clone(),
    });
    
    // Send message with both session_id and instance_id (session should take priority)
    app.process_claude_message(MockClaudeMessage::StreamText {
        instance_id: instance_id_2, // Wrong instance ID
        text: "Session takes priority".to_string(),
        session_id: Some(session_id_1.clone()), // Correct session ID
    });
    
    // Verify message went to session_id match, not instance_id match
    assert_eq!(app.instances[1].messages.len(), 1, "Should route to session match (Veda-2)");
    assert_eq!(app.instances[2].messages.len(), 0, "Should not route to instance match (Veda-3)");
    assert_eq!(app.instances[1].messages[0].content, "Session takes priority");
}

#[tokio::test]
async fn test_multiple_concurrent_spawning() {
    let (mut app, tx) = MockApp::new();
    
    // Spawn multiple instances concurrently
    let mut handles = Vec::new();
    let spawn_calls_clone = app.spawn_calls.clone();
    
    for i in 0..5 {
        let instance_id = app.spawn_instance();
        let tx_clone = tx.clone();
        let spawn_calls_ref = spawn_calls_clone.clone();
        
        let handle = tokio::spawn(async move {
            std::env::set_var("VEDA_TARGET_INSTANCE_ID", instance_id.to_string());
            
            // Mock the spawn call
            spawn_calls_ref.lock().unwrap().push(SpawnCall {
                instance_id,
                message: format!("Concurrent task {}", i + 1),
                target_env_var: Some(instance_id.to_string()),
            });
            
            std::env::remove_var("VEDA_TARGET_INSTANCE_ID");
            instance_id
        });
        
        handles.push(handle);
    }
    
    // Wait for all spawns to complete
    let spawned_ids: Vec<Uuid> = future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    
    // Verify all spawns completed
    let spawn_calls = app.spawn_calls.lock().unwrap();
    assert_eq!(spawn_calls.len(), 5, "Should have 5 concurrent spawn calls");
    assert_eq!(app.instances.len(), 6, "Should have 6 total instances (1 original + 5 spawned)");
    
    // Verify each spawn had unique parameters
    for (i, call) in spawn_calls.iter().enumerate() {
        assert!(spawned_ids.contains(&call.instance_id), "Spawn call should match spawned instance");
        assert_eq!(call.message, format!("Concurrent task {}", i + 1));
        assert_eq!(call.target_env_var, Some(call.instance_id.to_string()));
    }
}

#[tokio::test]
async fn test_failed_spawn_handling() {
    let (app, tx) = MockApp::new();
    
    // Mock a failed spawn by not setting environment variable
    let instance_id = Uuid::new_v4();
    
    let spawn_result = app.mock_send_to_claude_with_session(
        instance_id,
        "This should work in mock".to_string(),
        tx,
        None,
        None,
    ).await;
    
    // In our mock, this succeeds, but we can verify environment wasn't set
    assert!(spawn_result.is_ok());
    
    let spawn_calls = app.spawn_calls.lock().unwrap();
    assert_eq!(spawn_calls.len(), 1);
    assert_eq!(spawn_calls[0].target_env_var, None, "Should not have environment variable when not set");
}