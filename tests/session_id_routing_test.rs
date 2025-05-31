use uuid::Uuid;

// Mock structures to test session_id routing functionality
#[derive(Clone)]
struct MockInstance {
    id: Uuid,
    name: String,
    session_id: Option<String>,
    messages: Vec<MockMessage>,
}

#[derive(Clone, Debug)]
struct MockMessage {
    sender: String,
    content: String,
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
    
    fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
    
    fn add_message(&mut self, sender: String, content: String) {
        self.messages.push(MockMessage { sender, content });
    }
}

struct MockApp {
    instances: Vec<MockInstance>,
}

impl MockApp {
    fn new() -> Self {
        Self {
            instances: vec![MockInstance::new("Claude 1".to_string())],
        }
    }
    
    // Simulate the session_id routing logic from main.rs
    fn find_instance_by_session_or_id(&self, instance_id: Uuid, session_id: Option<&String>) -> Option<usize> {
        if let Some(session_id_val) = session_id {
            // First try to find by session_id (for spawned instances)
            self.instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                .or_else(|| self.instances.iter().position(|i| i.id == instance_id))
        } else {
            // Fallback to instance_id
            self.instances.iter().position(|i| i.id == instance_id)
        }
    }
    
    fn simulate_stream_text(&mut self, instance_id: Uuid, text: String, session_id: Option<String>) -> bool {
        let target_instance_index = self.find_instance_by_session_or_id(instance_id, session_id.as_ref());
        
        if let Some(instance_idx) = target_instance_index {
            self.instances[instance_idx].add_message("Claude".to_string(), text);
            true
        } else {
            false
        }
    }
}

#[test]
fn test_session_id_routing_preference() {
    // Test that session_id takes precedence over instance_id
    let mut app = MockApp::new();
    
    // Create instances with different session_ids
    let instance_2_id = Uuid::new_v4();
    let instance_3_id = Uuid::new_v4();
    
    let instance_2 = MockInstance::new("Claude 2".to_string())
        .with_session_id("session-alpha".to_string());
    let instance_3 = MockInstance::new("Claude 3".to_string())
        .with_session_id("session-beta".to_string());
    
    // Set specific IDs for testing
    app.instances.push(MockInstance { id: instance_2_id, ..instance_2 });
    app.instances.push(MockInstance { id: instance_3_id, ..instance_3 });
    
    // Test 1: Message with session_id should go to correct instance regardless of instance_id
    let wrong_instance_id = Uuid::new_v4(); // Random ID that doesn't match any instance
    let success = app.simulate_stream_text(
        wrong_instance_id, 
        "Message for session-beta".to_string(), 
        Some("session-beta".to_string())
    );
    
    assert!(success, "Should find instance by session_id even with wrong instance_id");
    assert!(app.instances[2].messages.iter().any(|m| m.content.contains("session-beta")), 
           "Message should appear in instance with session-beta");
    assert!(!app.instances[1].messages.iter().any(|m| m.content.contains("session-beta")), 
           "Message should NOT appear in instance with session-alpha");
    
    println!("✅ Session ID routing preference works correctly");
}

#[test]
fn test_instance_id_fallback() {
    // Test that instance_id is used when no session_id is provided
    let mut app = MockApp::new();
    
    let instance_2_id = Uuid::new_v4();
    let instance_2 = MockInstance::new("Claude 2".to_string());
    app.instances.push(MockInstance { id: instance_2_id, ..instance_2 });
    
    // Send message without session_id
    let success = app.simulate_stream_text(
        instance_2_id, 
        "Message without session_id".to_string(), 
        None
    );
    
    assert!(success, "Should find instance by instance_id when no session_id");
    assert!(app.instances[1].messages.iter().any(|m| m.content.contains("without session_id")), 
           "Message should appear in correct instance");
    
    println!("✅ Instance ID fallback works correctly");
}

#[test]
fn test_session_id_override_instance_id() {
    // Test complex scenario where session_id and instance_id point to different instances
    let mut app = MockApp::new();
    
    let instance_2_id = Uuid::new_v4();
    let instance_3_id = Uuid::new_v4();
    
    // Instance 2 has session-alpha
    let instance_2 = MockInstance::new("Claude 2".to_string())
        .with_session_id("session-alpha".to_string());
    
    // Instance 3 has session-beta
    let instance_3 = MockInstance::new("Claude 3".to_string())
        .with_session_id("session-beta".to_string());
    
    app.instances.push(MockInstance { id: instance_2_id, ..instance_2 });
    app.instances.push(MockInstance { id: instance_3_id, ..instance_3 });
    
    // Send message with instance_2_id but session-beta (should go to instance 3)
    let success = app.simulate_stream_text(
        instance_2_id,  // Points to instance 2
        "Should go to instance 3".to_string(), 
        Some("session-beta".to_string())  // Points to instance 3
    );
    
    assert!(success, "Should successfully route message");
    
    // Message should go to instance 3 (session-beta), not instance 2 (instance_2_id)
    assert!(!app.instances[1].messages.iter().any(|m| m.content.contains("Should go to instance 3")), 
           "Message should NOT go to instance 2 despite matching instance_id");
    assert!(app.instances[2].messages.iter().any(|m| m.content.contains("Should go to instance 3")), 
           "Message should go to instance 3 based on session_id");
    
    println!("✅ Session ID override of instance ID works correctly");
}

#[test]
fn test_no_session_match_fallback_to_instance() {
    // Test that when session_id doesn't match any instance, it falls back to instance_id
    let mut app = MockApp::new();
    
    let instance_2_id = Uuid::new_v4();
    let instance_2 = MockInstance::new("Claude 2".to_string())
        .with_session_id("session-alpha".to_string());
    
    app.instances.push(MockInstance { id: instance_2_id, ..instance_2 });
    
    // Send message with non-existent session_id but valid instance_id
    let success = app.simulate_stream_text(
        instance_2_id, 
        "Fallback to instance_id".to_string(), 
        Some("non-existent-session".to_string())
    );
    
    assert!(success, "Should fallback to instance_id when session_id not found");
    assert!(app.instances[1].messages.iter().any(|m| m.content.contains("Fallback to instance_id")), 
           "Message should appear in instance found by instance_id");
    
    println!("✅ Session ID fallback to instance ID works correctly");
}

#[test]
fn test_neither_session_nor_instance_match() {
    // Test that when neither session_id nor instance_id match, routing fails gracefully
    let mut app = MockApp::new();
    
    let instance_2_id = Uuid::new_v4();
    let instance_2 = MockInstance::new("Claude 2".to_string())
        .with_session_id("session-alpha".to_string());
    
    app.instances.push(MockInstance { id: instance_2_id, ..instance_2 });
    
    // Send message with non-existent session_id and non-existent instance_id
    let wrong_instance_id = Uuid::new_v4();
    let success = app.simulate_stream_text(
        wrong_instance_id, 
        "Should not be routed".to_string(), 
        Some("non-existent-session".to_string())
    );
    
    assert!(!success, "Should fail when neither session_id nor instance_id match");
    
    // Verify no messages were added to any instance
    for instance in &app.instances {
        assert!(!instance.messages.iter().any(|m| m.content.contains("Should not be routed")), 
               "Message should not appear in any instance");
    }
    
    println!("✅ Graceful handling of unmatched routing works correctly");
}

#[test]
fn test_real_world_spawned_instance_scenario() {
    // Test a realistic scenario with main instance and spawned instances
    let mut app = MockApp::new();
    
    // Main instance (no session_id initially)
    let main_instance_id = app.instances[0].id;
    
    // Spawned instances with session_ids
    let spawned_1_id = Uuid::new_v4();
    let spawned_2_id = Uuid::new_v4();
    
    let spawned_1 = MockInstance::new("Claude 2".to_string())
        .with_session_id("76251a15-e564-449e-a810-f05a26ed782a".to_string());
    let spawned_2 = MockInstance::new("Claude 3".to_string())
        .with_session_id("89abc123-1234-5678-9abc-def012345678".to_string());
    
    app.instances.push(MockInstance { id: spawned_1_id, ..spawned_1 });
    app.instances.push(MockInstance { id: spawned_2_id, ..spawned_2 });
    
    // Test messages to different instances
    assert!(app.simulate_stream_text(
        main_instance_id, 
        "Message to main instance".to_string(), 
        None
    ), "Main instance should receive message");
    
    assert!(app.simulate_stream_text(
        spawned_1_id, 
        "Message to spawned instance 1".to_string(), 
        Some("76251a15-e564-449e-a810-f05a26ed782a".to_string())
    ), "Spawned instance 1 should receive message");
    
    assert!(app.simulate_stream_text(
        spawned_2_id, 
        "Message to spawned instance 2".to_string(), 
        Some("89abc123-1234-5678-9abc-def012345678".to_string())
    ), "Spawned instance 2 should receive message");
    
    // Verify messages went to correct instances
    assert!(app.instances[0].messages.iter().any(|m| m.content.contains("main instance")), 
           "Main instance should have its message");
    assert!(app.instances[1].messages.iter().any(|m| m.content.contains("spawned instance 1")), 
           "Spawned instance 1 should have its message");
    assert!(app.instances[2].messages.iter().any(|m| m.content.contains("spawned instance 2")), 
           "Spawned instance 2 should have its message");
    
    // Verify no cross-contamination
    assert!(!app.instances[0].messages.iter().any(|m| m.content.contains("spawned instance")), 
           "Main instance should not have spawned instance messages");
    assert!(!app.instances[1].messages.iter().any(|m| m.content.contains("main instance")), 
           "Spawned instance 1 should not have main instance message");
    assert!(!app.instances[2].messages.iter().any(|m| m.content.contains("spawned instance 1")), 
           "Spawned instance 2 should not have spawned instance 1 message");
    
    println!("✅ Real-world spawned instance scenario works correctly");
}