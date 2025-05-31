#[cfg(test)]
mod session_routing_tests {
    use uuid::Uuid;
    use std::collections::HashMap;
    use tokio::sync::mpsc;
    use serde_json::json;
    
    // Simulated message types matching the actual ClaudeMessage enum
    #[derive(Debug, Clone)]
    enum MockClaudeMessage {
        StreamText {
            instance_id: Uuid,
            text: String,
            session_id: Option<String>,
        },
        VedaSpawnInstances {
            instance_id: Uuid,
            task_description: String,
            num_instances: i64,
            is_ipc: bool,
        },
        VedaListInstances {
            instance_id: Uuid,
        },
        SessionStart {
            instance_id: Uuid,
            session_id: String,
        },
        InternalCoordinateInstances {
            main_instance_id: Uuid,
            task_description: String,
            num_instances: usize,
            working_dir: String,
            is_ipc: bool,
        },
    }
    
    #[derive(Debug)]
    struct MockInstance {
        id: Uuid,
        name: String,
        session_id: Option<String>,
        messages: Vec<(String, String)>, // (sender, content)
        is_processing: bool,
    }
    
    struct MockRoutingSystem {
        instances: Vec<MockInstance>,
        session_map: HashMap<String, usize>, // session_id -> instance index
    }
    
    impl MockRoutingSystem {
        fn new() -> Self {
            let mut system = Self {
                instances: Vec::new(),
                session_map: HashMap::new(),
            };
            
            // Create main instance
            system.instances.push(MockInstance {
                id: Uuid::new_v4(),
                name: "Veda-1".to_string(),
                session_id: None,
                messages: Vec::new(),
                is_processing: false,
            });
            
            system
        }
        
        fn spawn_instance(&mut self) -> (usize, Uuid, String) {
            let idx = self.instances.len();
            let id = Uuid::new_v4();
            let name = format!("Veda-{}", idx + 1);
            let session_id = format!("session-{}-{}", idx, Uuid::new_v4());
            
            self.instances.push(MockInstance {
                id,
                name: name.clone(),
                session_id: Some(session_id.clone()),
                messages: Vec::new(),
                is_processing: false,
            });
            
            self.session_map.insert(session_id.clone(), idx);
            (idx, id, session_id)
        }
        
        fn route_message(&mut self, msg: MockClaudeMessage) -> Option<usize> {
            match msg {
                MockClaudeMessage::StreamText { instance_id, text, session_id } => {
                    // First try session_id, then fall back to instance_id
                    let idx = if let Some(sid) = &session_id {
                        self.session_map.get(sid).copied()
                    } else {
                        None
                    }.or_else(|| {
                        self.instances.iter().position(|i| i.id == instance_id)
                    });
                    
                    if let Some(idx) = idx {
                        self.instances[idx].messages.push(("Claude".to_string(), text));
                    }
                    
                    idx
                }
                _ => None,
            }
        }
        
        fn assign_session_to_instance(&mut self, instance_idx: usize, session_id: String) {
            if let Some(instance) = self.instances.get_mut(instance_idx) {
                instance.session_id = Some(session_id.clone());
                self.session_map.insert(session_id, instance_idx);
            }
        }
    }
    
    #[test]
    fn test_session_routing_priority() {
        let mut routing = MockRoutingSystem::new();
        
        // Spawn two instances
        let (idx1, id1, session1) = routing.spawn_instance();
        let (idx2, id2, session2) = routing.spawn_instance();
        
        // Test 1: Route by session_id (should take priority)
        let msg1 = MockClaudeMessage::StreamText {
            instance_id: id1, // Wrong instance ID
            text: "Message for instance 2".to_string(),
            session_id: Some(session2.clone()), // But correct session ID
        };
        
        let routed_idx = routing.route_message(msg1);
        assert_eq!(routed_idx, Some(idx2)); // Should route to instance 2
        assert_eq!(routing.instances[idx2].messages.len(), 1);
        assert_eq!(routing.instances[idx2].messages[0].1, "Message for instance 2");
        
        // Test 2: Route by instance_id when no session_id
        let msg2 = MockClaudeMessage::StreamText {
            instance_id: id1,
            text: "Message for instance 1".to_string(),
            session_id: None,
        };
        
        let routed_idx = routing.route_message(msg2);
        assert_eq!(routed_idx, Some(idx1));
        assert_eq!(routing.instances[idx1].messages.len(), 1);
        assert_eq!(routing.instances[idx1].messages[0].1, "Message for instance 1");
    }
    
    #[test]
    fn test_session_assignment_after_spawn() {
        let mut routing = MockRoutingSystem::new();
        
        // Spawn instance without session initially
        let idx = routing.instances.len();
        let id = Uuid::new_v4();
        routing.instances.push(MockInstance {
            id,
            name: format!("Veda-{}", idx + 1),
            session_id: None,
            messages: Vec::new(),
            is_processing: false,
        });
        
        // Try to route message - should fail
        let msg1 = MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(), // Wrong ID
            text: "Lost message".to_string(),
            session_id: Some("session-not-assigned".to_string()),
        };
        
        let routed = routing.route_message(msg1);
        assert_eq!(routed, None);
        
        // Now assign session
        let session_id = "session-newly-assigned";
        routing.assign_session_to_instance(idx, session_id.to_string());
        
        // Now routing should work
        let msg2 = MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(), // Still wrong ID
            text: "Found message".to_string(),
            session_id: Some(session_id.to_string()),
        };
        
        let routed = routing.route_message(msg2);
        assert_eq!(routed, Some(idx));
        assert_eq!(routing.instances[idx].messages.len(), 1);
        assert_eq!(routing.instances[idx].messages[0].1, "Found message");
    }
    
    #[tokio::test]
    async fn test_concurrent_session_routing() {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        
        let routing = Arc::new(Mutex::new(MockRoutingSystem::new()));
        
        // Spawn multiple instances
        let mut sessions = Vec::new();
        {
            let mut r = routing.lock().await;
            for _ in 0..3 {
                let (_, _, session) = r.spawn_instance();
                sessions.push(session);
            }
        }
        
        // Send concurrent messages
        let mut handles = vec![];
        
        for (i, session) in sessions.iter().enumerate() {
            let routing_clone = routing.clone();
            let session_clone = session.clone();
            let instance_num = i + 2; // Veda-2, Veda-3, Veda-4
            
            let handle = tokio::spawn(async move {
                for msg_idx in 0..10 {
                    let msg = MockClaudeMessage::StreamText {
                        instance_id: Uuid::new_v4(), // Random ID to test session routing
                        text: format!("Message {} for Veda-{}", msg_idx, instance_num),
                        session_id: Some(session_clone.clone()),
                    };
                    
                    let mut r = routing_clone.lock().await;
                    r.route_message(msg);
                    drop(r);
                    
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
            });
            handles.push(handle);
        }
        
        // Wait for completion
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify routing
        let r = routing.lock().await;
        for i in 1..=3 {
            assert_eq!(r.instances[i].messages.len(), 10);
            let instance_num = i + 1;
            for (j, (_, msg)) in r.instances[i].messages.iter().enumerate() {
                assert_eq!(msg, &format!("Message {} for Veda-{}", j, instance_num));
            }
        }
    }
    
    #[test]
    fn test_session_cleanup_on_instance_close() {
        let mut routing = MockRoutingSystem::new();
        
        // Spawn instances
        let (idx1, _, session1) = routing.spawn_instance();
        let (idx2, _, session2) = routing.spawn_instance();
        let (idx3, _, session3) = routing.spawn_instance();
        
        // Verify initial state
        assert_eq!(routing.instances.len(), 4); // Main + 3 spawned
        assert_eq!(routing.session_map.len(), 3);
        
        // Remove middle instance (simulating close)
        routing.instances.remove(idx2);
        routing.session_map.remove(&session2);
        
        // Update remaining session mappings (indices shifted)
        routing.session_map.clear();
        for (i, instance) in routing.instances.iter().enumerate() {
            if let Some(ref session) = instance.session_id {
                routing.session_map.insert(session.clone(), i);
            }
        }
        
        // Verify sessions still route correctly
        let msg1 = MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(),
            text: "Message for first spawned".to_string(),
            session_id: Some(session1),
        };
        
        let msg3 = MockClaudeMessage::StreamText {
            instance_id: Uuid::new_v4(),
            text: "Message for third spawned".to_string(),
            session_id: Some(session3),
        };
        
        let routed1 = routing.route_message(msg1);
        let routed3 = routing.route_message(msg3);
        
        assert_eq!(routed1, Some(1)); // Now at index 1
        assert_eq!(routed3, Some(2)); // Now at index 2
    }
    
    #[test]
    fn test_mcp_spawn_workflow() {
        let mut routing = MockRoutingSystem::new();
        let main_id = routing.instances[0].id;
        
        // Simulate MCP spawn request
        let spawn_msg = MockClaudeMessage::VedaSpawnInstances {
            instance_id: main_id,
            task_description: "Build authentication system".to_string(),
            num_instances: 2,
            is_ipc: false,
        };
        
        // Process spawn (in real app this would trigger spawn_coordinated_instances)
        match spawn_msg {
            MockClaudeMessage::VedaSpawnInstances { num_instances, task_description, .. } => {
                for i in 0..num_instances {
                    let (idx, _, session) = routing.spawn_instance();
                    routing.instances[idx].messages.push((
                        "System".to_string(),
                        format!("Coordination: Task {} of {}: {}", i + 1, num_instances, task_description)
                    ));
                }
            }
            _ => {}
        }
        
        // Verify spawned instances
        assert_eq!(routing.instances.len(), 3); // Main + 2 spawned
        assert_eq!(routing.instances[1].name, "Veda-2");
        assert_eq!(routing.instances[2].name, "Veda-3");
        
        // Verify coordination messages
        assert!(routing.instances[1].messages[0].1.contains("Task 1 of 2"));
        assert!(routing.instances[2].messages[0].1.contains("Task 2 of 2"));
        assert!(routing.instances[1].messages[0].1.contains("Build authentication system"));
    }
    
    #[test]
    fn test_main_instance_no_session() {
        let routing = MockRoutingSystem::new();
        
        // Main instance should never have a session ID
        assert!(routing.instances[0].session_id.is_none());
        assert_eq!(routing.instances[0].name, "Veda-1");
        
        // Session map should not contain main instance
        assert_eq!(routing.session_map.len(), 0);
    }
    
    #[test]
    fn test_session_id_format() {
        let mut routing = MockRoutingSystem::new();
        
        // Spawn several instances
        for _ in 0..3 {
            routing.spawn_instance();
        }
        
        // Check session ID format
        for (i, instance) in routing.instances.iter().enumerate().skip(1) {
            if let Some(ref session) = instance.session_id {
                assert!(session.starts_with(&format!("session-{}-", i)));
                // Should have UUID suffix
                let parts: Vec<&str> = session.split('-').collect();
                assert!(parts.len() >= 6); // "session-INDEX-UUID" format
            }
        }
    }
}