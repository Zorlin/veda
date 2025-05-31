#[cfg(test)]
mod session_id_routing_tests {
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};
    use uuid::Uuid;
    use std::collections::HashMap;
    
    // Simulate the ClaudeMessage enum for routing
    #[derive(Debug, Clone)]
    enum MockClaudeMessage {
        StreamText {
            instance_id: Uuid,
            text: String,
            session_id: Option<String>,
        },
        SessionStarted {
            instance_id: Uuid,
            session_id: String,
        },
        ToolUse {
            instance_id: Uuid,
            tool_name: String,
            session_id: Option<String>,
        },
    }
    
    // Mock instance for testing
    #[derive(Debug, Clone)]
    struct MockInstance {
        id: Uuid,
        name: String,
        session_id: Option<String>,
        messages: Vec<(String, String)>, // (sender, content)
    }
    
    impl MockInstance {
        fn new(name: String) -> Self {
            Self {
                id: Uuid::new_v4(),
                name,
                session_id: None,
                messages: Vec::new(),
            }
        }
        
        fn add_message(&mut self, sender: String, content: String) {
            self.messages.push((sender, content));
        }
    }
    
    // Mock app to test routing logic
    struct MockApp {
        instances: Vec<MockInstance>,
        message_tx: mpsc::Sender<MockClaudeMessage>,
        message_rx: mpsc::Receiver<MockClaudeMessage>,
    }
    
    impl MockApp {
        fn new() -> Self {
            let (tx, rx) = mpsc::channel(100);
            let mut app = Self {
                instances: Vec::new(),
                message_tx: tx,
                message_rx: rx,
            };
            // Create main instance
            app.instances.push(MockInstance::new("Veda-1".to_string()));
            app
        }
        
        fn spawn_instance(&mut self) -> (usize, Uuid) {
            let name = format!("Veda-{}", self.instances.len() + 1);
            let instance = MockInstance::new(name);
            let id = instance.id;
            self.instances.push(instance);
            (self.instances.len() - 1, id)
        }
        
        async fn process_messages(&mut self) {
            while let Ok(msg) = self.message_rx.try_recv() {
                match msg {
                    MockClaudeMessage::StreamText { instance_id, text, session_id } => {
                        // This mimics the routing logic from main.rs
                        let target_instance_index = if let Some(session_id_val) = &session_id {
                            // First try to find by session_id (for spawned instances)
                            self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                                .or_else(|| self.instances.iter().position(|i| i.id == instance_id))
                        } else {
                            // Fallback to instance_id
                            self.instances.iter().position(|i| i.id == instance_id)
                        };
                        
                        if let Some(idx) = target_instance_index {
                            self.instances[idx].add_message("Claude".to_string(), text);
                        }
                    }
                    MockClaudeMessage::SessionStarted { instance_id, session_id } => {
                        // Assign session ID to instance
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                            instance.session_id = Some(session_id);
                        }
                    }
                    MockClaudeMessage::ToolUse { instance_id, tool_name, session_id } => {
                        let target_instance_index = if let Some(session_id_val) = &session_id {
                            self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                                .or_else(|| self.instances.iter().position(|i| i.id == instance_id))
                        } else {
                            self.instances.iter().position(|i| i.id == instance_id)
                        };
                        
                        if let Some(idx) = target_instance_index {
                            self.instances[idx].add_message("Tool".to_string(), tool_name);
                        }
                    }
                }
            }
        }
    }
    
    #[tokio::test]
    async fn test_session_id_assignment_on_spawn() {
        let mut app = MockApp::new();
        
        // Spawn instances
        let (idx1, id1) = app.spawn_instance();
        let (idx2, id2) = app.spawn_instance();
        let (idx3, id3) = app.spawn_instance();
        
        // Simulate session start events
        app.message_tx.send(MockClaudeMessage::SessionStarted {
            instance_id: id1,
            session_id: "session-abc-123".to_string(),
        }).await.unwrap();
        
        app.message_tx.send(MockClaudeMessage::SessionStarted {
            instance_id: id2,
            session_id: "session-def-456".to_string(),
        }).await.unwrap();
        
        app.message_tx.send(MockClaudeMessage::SessionStarted {
            instance_id: id3,
            session_id: "session-ghi-789".to_string(),
        }).await.unwrap();
        
        // Process messages
        app.process_messages().await;
        
        // Verify session IDs were assigned
        assert_eq!(app.instances[idx1].session_id, Some("session-abc-123".to_string()));
        assert_eq!(app.instances[idx2].session_id, Some("session-def-456".to_string()));
        assert_eq!(app.instances[idx3].session_id, Some("session-ghi-789".to_string()));
        
        // Main instance should not have session ID
        assert!(app.instances[0].session_id.is_none());
    }
    
    #[tokio::test]
    async fn test_message_routing_by_session_id() {
        let mut app = MockApp::new();
        
        // Spawn instances and assign session IDs
        let (_, id1) = app.spawn_instance();
        let (_, id2) = app.spawn_instance();
        
        app.instances[1].session_id = Some("session-tab2".to_string());
        app.instances[2].session_id = Some("session-tab3".to_string());
        
        // Send messages with session IDs
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(), // Wrong instance ID
            text: "This should go to Tab 2".to_string(),
            session_id: Some("session-tab2".to_string()),
        }).await.unwrap();
        
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(), // Wrong instance ID
            text: "This should go to Tab 3".to_string(),
            session_id: Some("session-tab3".to_string()),
        }).await.unwrap();
        
        // Process messages
        app.process_messages().await;
        
        // Verify messages were routed correctly by session ID
        assert_eq!(app.instances[1].messages.len(), 1);
        assert_eq!(app.instances[1].messages[0].1, "This should go to Tab 2");
        
        assert_eq!(app.instances[2].messages.len(), 1);
        assert_eq!(app.instances[2].messages[0].1, "This should go to Tab 3");
        
        // Main instance should have no messages
        assert_eq!(app.instances[0].messages.len(), 0);
    }
    
    #[tokio::test]
    async fn test_fallback_to_instance_id_routing() {
        let mut app = MockApp::new();
        
        let (_, id1) = app.spawn_instance();
        
        // Send message without session ID
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: id1,
            text: "Fallback routing test".to_string(),
            session_id: None,
        }).await.unwrap();
        
        // Process messages
        app.process_messages().await;
        
        // Should route by instance ID
        assert_eq!(app.instances[1].messages.len(), 1);
        assert_eq!(app.instances[1].messages[0].1, "Fallback routing test");
    }
    
    #[tokio::test]
    async fn test_concurrent_message_routing() {
        let app = Arc::new(Mutex::new(MockApp::new()));
        
        // Setup instances with session IDs
        {
            let mut app_guard = app.lock().await;
            app_guard.spawn_instance();
            app_guard.spawn_instance();
            app_guard.spawn_instance();
            
            app_guard.instances[1].session_id = Some("session-worker-1".to_string());
            app_guard.instances[2].session_id = Some("session-worker-2".to_string());
            app_guard.instances[3].session_id = Some("session-worker-3".to_string());
        }
        
        // Spawn concurrent senders
        let mut handles = vec![];
        
        for i in 1..=3 {
            let app_clone = app.clone();
            let session_id = format!("session-worker-{}", i);
            
            let handle = tokio::spawn(async move {
                let tx = {
                    let app_guard = app_clone.lock().await;
                    app_guard.message_tx.clone()
                };
                
                for msg_num in 0..10 {
                    tx.send(MockClaudeMessage::StreamText {
                        instance_id: Uuid::new_v4(), // Random ID to test session routing
                        text: format!("Worker {} Message {}", i, msg_num),
                        session_id: Some(session_id.clone()),
                    }).await.unwrap();
                    
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
            });
            handles.push(handle);
        }
        
        // Wait for all senders
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Process all messages
        {
            let mut app_guard = app.lock().await;
            app_guard.process_messages().await;
        }
        
        // Verify all messages were routed correctly
        let app_guard = app.lock().await;
        for i in 1..=3 {
            assert_eq!(app_guard.instances[i].messages.len(), 10);
            // Check first and last messages
            assert_eq!(app_guard.instances[i].messages[0].1, format!("Worker {} Message 0", i));
            assert_eq!(app_guard.instances[i].messages[9].1, format!("Worker {} Message 9", i));
        }
        
        // Main instance should have no messages
        assert_eq!(app_guard.instances[0].messages.len(), 0);
    }
    
    #[tokio::test]
    async fn test_tool_use_routing() {
        let mut app = MockApp::new();
        
        let (_, id1) = app.spawn_instance();
        app.instances[1].session_id = Some("session-tools".to_string());
        
        // Send tool use with session ID
        app.message_tx.send(MockClaudeMessage::ToolUse {
            instance_id: Uuid::new_v4(), // Wrong instance ID
            tool_name: "veda_spawn_instances".to_string(),
            session_id: Some("session-tools".to_string()),
        }).await.unwrap();
        
        // Send another tool use with correct instance ID
        app.message_tx.send(MockClaudeMessage::ToolUse {
            instance_id: id1,
            tool_name: "veda_list_instances".to_string(),
            session_id: None,
        }).await.unwrap();
        
        // Process messages
        app.process_messages().await;
        
        // Both should go to the same instance
        assert_eq!(app.instances[1].messages.len(), 2);
        assert_eq!(app.instances[1].messages[0], ("Tool".to_string(), "veda_spawn_instances".to_string()));
        assert_eq!(app.instances[1].messages[1], ("Tool".to_string(), "veda_list_instances".to_string()));
    }
    
    #[test]
    fn test_session_id_uniqueness() {
        let mut sessions = HashMap::new();
        
        // Simulate creating multiple session IDs
        for i in 0..100 {
            let session_id = format!("session-{}-{}", i, Uuid::new_v4());
            assert!(sessions.insert(session_id.clone(), i).is_none());
        }
        
        // All should be unique
        assert_eq!(sessions.len(), 100);
    }
    
    #[tokio::test]
    async fn test_late_session_assignment() {
        let mut app = MockApp::new();
        
        let (idx, id) = app.spawn_instance();
        
        // Send message before session is assigned
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: id,
            text: "Message before session".to_string(),
            session_id: None,
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Verify message was routed by instance ID
        assert_eq!(app.instances[idx].messages.len(), 1);
        
        // Now assign session
        app.message_tx.send(MockClaudeMessage::SessionStarted {
            instance_id: id,
            session_id: "session-late".to_string(),
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Send message with session ID
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(), // Different instance ID
            text: "Message after session".to_string(),
            session_id: Some("session-late".to_string()),
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Should have both messages
        assert_eq!(app.instances[idx].messages.len(), 2);
        assert_eq!(app.instances[idx].messages[0].1, "Message before session");
        assert_eq!(app.instances[idx].messages[1].1, "Message after session");
    }
    
    #[test]
    fn test_session_id_format() {
        // Test expected session ID formats
        let patterns = vec![
            "session-abc-123",
            "session-1-7f8b9c0d-1234-5678-9abc-def012345678",
            "session-worker-42",
            "session-test-integration",
        ];
        
        for pattern in patterns {
            assert!(pattern.starts_with("session-"));
            assert!(pattern.len() > 8); // At least "session-X"
        }
    }
    
    #[tokio::test]
    async fn test_session_priority_over_instance_id() {
        let mut app = MockApp::new();
        
        // Create two instances
        let (_, id1) = app.spawn_instance();
        let (_, id2) = app.spawn_instance();
        
        // Assign session IDs
        app.instances[1].session_id = Some("session-alpha".to_string());
        app.instances[2].session_id = Some("session-beta".to_string());
        
        // Send message with instance_id pointing to instance 1, but session_id pointing to instance 2
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: id1, // Points to Veda-2
            text: "Priority test message".to_string(),
            session_id: Some("session-beta".to_string()), // Points to Veda-3
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Message should go to instance 2 (session-beta), not instance 1
        assert_eq!(app.instances[1].messages.len(), 0);
        assert_eq!(app.instances[2].messages.len(), 1);
        assert_eq!(app.instances[2].messages[0].1, "Priority test message");
    }
    
    #[tokio::test]
    async fn test_missing_session_fallback() {
        let mut app = MockApp::new();
        
        let (_, id1) = app.spawn_instance();
        app.instances[1].session_id = Some("session-exists".to_string());
        
        // Send with non-existent session but valid instance ID
        app.message_tx.send(MockClaudeMessage::StreamText {
            instance_id: id1,
            text: "Fallback message".to_string(),
            session_id: Some("session-does-not-exist".to_string()),
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Should fallback to instance ID
        assert_eq!(app.instances[1].messages.len(), 1);
        assert_eq!(app.instances[1].messages[0].1, "Fallback message");
    }
}