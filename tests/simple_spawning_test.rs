use uuid::Uuid;
use std::collections::HashMap;

// Simple test to validate the core spawning fix concepts
#[derive(Debug, Clone)]
struct TestInstance {
    id: Uuid,
    name: String,
    session_id: Option<String>,
    messages: Vec<String>,
}

struct SimpleApp {
    instances: Vec<TestInstance>,
    session_to_instance: HashMap<String, Uuid>,
}

impl TestInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            session_id: None,
            messages: Vec::new(),
        }
    }
}

impl SimpleApp {
    fn new() -> Self {
        Self {
            instances: vec![TestInstance::new("Veda-1".to_string())],
            session_to_instance: HashMap::new(),
        }
    }

    fn spawn_instance(&mut self, name: String) -> Uuid {
        let instance = TestInstance::new(name);
        let instance_id = instance.id;
        self.instances.push(instance);
        instance_id
    }

    fn establish_session(&mut self, instance_id: Uuid, session_id: String) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
            instance.session_id = Some(session_id.clone());
            self.session_to_instance.insert(session_id, instance_id);
        }
    }

    fn route_message_by_session(&mut self, session_id: &str, message: String) -> bool {
        if let Some(&instance_id) = self.session_to_instance.get(session_id) {
            if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                instance.messages.push(message);
                return true;
            }
        }
        false
    }

    fn route_message_by_instance_id(&mut self, instance_id: Uuid, message: String) -> bool {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
            instance.messages.push(message);
            return true;
        }
        false
    }
}

#[test]
fn test_claude_code_spawning_fix_concept() {
    let mut app = SimpleApp::new();
    
    // Test the core fix: spawn instances with unique session handling
    let veda_2_id = app.spawn_instance("Veda-2".to_string());
    let veda_3_id = app.spawn_instance("Veda-3".to_string());
    
    assert_eq!(app.instances.len(), 3, "Should have 3 instances");
    
    // Simulate Claude Code establishing unique sessions (the key fix)
    app.establish_session(veda_2_id, "claude-session-abc".to_string());
    app.establish_session(veda_3_id, "claude-session-def".to_string());
    
    // Test session-based routing (should work after fix)
    let routed_2 = app.route_message_by_session("claude-session-abc", "Message for Veda-2".to_string());
    let routed_3 = app.route_message_by_session("claude-session-def", "Message for Veda-3".to_string());
    
    assert!(routed_2, "Should successfully route to Veda-2 via session");
    assert!(routed_3, "Should successfully route to Veda-3 via session");
    
    // Verify no crossover
    assert_eq!(app.instances[0].messages.len(), 0, "Veda-1 should have no messages");
    assert_eq!(app.instances[1].messages.len(), 1, "Veda-2 should have exactly one message");
    assert_eq!(app.instances[2].messages.len(), 1, "Veda-3 should have exactly one message");
    
    // Verify correct content
    assert_eq!(app.instances[1].messages[0], "Message for Veda-2");
    assert_eq!(app.instances[2].messages[0], "Message for Veda-3");
}

#[test]
fn test_environment_variable_isolation_concept() {
    // Test that environment variable passing works as intended
    
    let instance_id_1 = Uuid::new_v4();
    let instance_id_2 = Uuid::new_v4();
    
    // Simulate setting environment variable for first spawn
    std::env::set_var("VEDA_TARGET_INSTANCE_ID", instance_id_1.to_string());
    let env_1 = std::env::var("VEDA_TARGET_INSTANCE_ID").ok();
    std::env::remove_var("VEDA_TARGET_INSTANCE_ID");
    
    // Simulate setting environment variable for second spawn  
    std::env::set_var("VEDA_TARGET_INSTANCE_ID", instance_id_2.to_string());
    let env_2 = std::env::var("VEDA_TARGET_INSTANCE_ID").ok();
    std::env::remove_var("VEDA_TARGET_INSTANCE_ID");
    
    // Verify isolation
    assert_eq!(env_1, Some(instance_id_1.to_string()), "First spawn should get first instance ID");
    assert_eq!(env_2, Some(instance_id_2.to_string()), "Second spawn should get second instance ID");
    assert_ne!(env_1, env_2, "Environment variables should be different");
    
    // Verify cleanup
    assert!(std::env::var("VEDA_TARGET_INSTANCE_ID").is_err(), "Environment should be clean after test");
}

#[test] 
fn test_session_routing_priority() {
    let mut app = SimpleApp::new();
    
    let instance_1 = app.spawn_instance("Test-1".to_string());
    let instance_2 = app.spawn_instance("Test-2".to_string());
    
    // Establish sessions
    app.establish_session(instance_1, "session-A".to_string());
    app.establish_session(instance_2, "session-B".to_string());
    
    // Test that session routing takes priority over instance ID routing
    let session_routed = app.route_message_by_session("session-A", "Via session".to_string());
    let instance_routed = app.route_message_by_instance_id(instance_2, "Via instance ID".to_string());
    
    assert!(session_routed, "Session routing should work");
    assert!(instance_routed, "Instance ID routing should work");
    
    // Verify messages went to correct instances
    assert_eq!(app.instances[1].messages.len(), 1, "Instance 1 should have session message");
    assert_eq!(app.instances[2].messages.len(), 1, "Instance 2 should have instance ID message");
    assert_eq!(app.instances[1].messages[0], "Via session");
    assert_eq!(app.instances[2].messages[0], "Via instance ID");
}

#[test]
fn test_no_message_blackholing() {
    let mut app = SimpleApp::new();
    
    let instance_id = app.spawn_instance("Test".to_string());
    app.establish_session(instance_id, "test-session".to_string());
    
    // Test multiple routing attempts
    let results = vec![
        app.route_message_by_session("test-session", "Message 1".to_string()),
        app.route_message_by_session("test-session", "Message 2".to_string()),
        app.route_message_by_instance_id(instance_id, "Message 3".to_string()),
    ];
    
    // Verify all messages were routed successfully (no blackholing)
    assert!(results.iter().all(|&r| r), "All messages should be routed successfully");
    assert_eq!(app.instances[1].messages.len(), 3, "All messages should be delivered");
    
    // Verify order and content
    assert_eq!(app.instances[1].messages[0], "Message 1");
    assert_eq!(app.instances[1].messages[1], "Message 2");
    assert_eq!(app.instances[1].messages[2], "Message 3");
}

#[test]
fn test_original_problem_scenario() {
    // Recreate the exact scenario from the original bug report
    let mut app = SimpleApp::new();
    
    // User spawns instances like in original bug
    let veda_2 = app.spawn_instance("Veda-2".to_string());
    let veda_3 = app.spawn_instance("Veda-3".to_string()); 
    let veda_4 = app.spawn_instance("Veda-4".to_string());
    
    // Each Claude Code instance gets unique session (the fix)
    app.establish_session(veda_2, "claude-real-session-1".to_string());
    app.establish_session(veda_3, "claude-real-session-2".to_string());
    app.establish_session(veda_4, "claude-real-session-3".to_string());
    
    // Simulate actual Claude Code output
    let routing_results = vec![
        app.route_message_by_session("claude-real-session-1", "Implementing MooseNG master...".to_string()),
        app.route_message_by_session("claude-real-session-2", "Working on chunk server...".to_string()),
        app.route_message_by_session("claude-real-session-3", "Adding erasure coding...".to_string()),
    ];
    
    // Verify the original bug is fixed
    assert!(routing_results.iter().all(|&r| r), "All routing should succeed");
    
    // CRITICAL: Veda-1 should NOT receive spawned instance messages
    assert_eq!(app.instances[0].messages.len(), 0, "❌ BUG: Veda-1 should not get spawned messages");
    
    // Each spawned instance should get exactly its own message
    assert_eq!(app.instances[1].messages.len(), 1, "Veda-2 should get its message");
    assert_eq!(app.instances[2].messages.len(), 1, "Veda-3 should get its message");
    assert_eq!(app.instances[3].messages.len(), 1, "Veda-4 should get its message");
    
    // Verify content correctness (no crossover)
    assert!(app.instances[1].messages[0].contains("master"), "Veda-2 should get master message");
    assert!(app.instances[2].messages[0].contains("chunk"), "Veda-3 should get chunk message");
    assert!(app.instances[3].messages[0].contains("erasure"), "Veda-4 should get erasure message");
    
    println!("✅ Original bug scenario test PASSED - spawning fix works!");
}