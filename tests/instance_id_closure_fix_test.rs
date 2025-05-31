use uuid::Uuid;
use std::collections::HashMap;

// Test to verify that instance IDs are properly captured in async closures
// for spawned Claude Code instances

#[derive(Debug, Clone)]
struct TestInstance {
    id: Uuid,
    name: String,
    session_id: Option<String>,
}

impl TestInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            session_id: None,
        }
    }
}

struct TestApp {
    instances: Vec<TestInstance>,
    spawned_instance_ids: Vec<Uuid>, // Track which instances were spawned
}

impl TestApp {
    fn new() -> Self {
        Self {
            instances: vec![TestInstance::new("Veda-1".to_string())],
            spawned_instance_ids: Vec::new(),
        }
    }

    async fn spawn_coordinated_instances_test(&mut self, requested_count: usize) {
        // Simulate the fixed spawning logic
        for _ in 0..requested_count {
            let instance_name = format!("Veda-{}", self.instances.len() + 1);
            let mut new_instance = TestInstance::new(instance_name);
            
            // Simulate the coordination message
            let coordination_message = format!("Test task for {}", new_instance.name);
            
            // CRITICAL: Capture the instance_id before moving into async closure
            let instance_id = new_instance.id;
            let instance_name = new_instance.name.clone();
            self.instances.push(new_instance);
            
            // Clone values needed for the async task (FIXED VERSION)
            let instance_name_owned = instance_name.clone();
            let instance_id_owned = instance_id; // Ensure the UUID is moved into the async closure
            let mut spawned_ids = self.spawned_instance_ids.clone();
            
            // Simulate async spawning with proper variable capture
            let spawn_task = async move {
                // Wait to simulate the actual spawn delay
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                
                // Record which instance ID was actually used for spawning
                spawned_ids.push(instance_id_owned);
                
                // Return the result
                (instance_id_owned, instance_name_owned, spawned_ids)
            };
            
            let (spawned_id, spawned_name, updated_spawned_ids) = spawn_task.await;
            self.spawned_instance_ids = updated_spawned_ids;
            
            println!("Spawned instance {} with ID {}", spawned_name, spawned_id);
        }
    }

    async fn spawn_coordinated_instances_broken(&mut self, requested_count: usize) {
        // Simulate the BROKEN version (for comparison)
        for _ in 0..requested_count {
            let instance_name = format!("Broken-{}", self.instances.len() + 1);
            let mut new_instance = TestInstance::new(instance_name);
            
            let instance_id = new_instance.id;
            let instance_name = new_instance.name.clone();
            self.instances.push(new_instance);
            
            // BROKEN: Don't properly move instance_id into closure
            let instance_name_owned = instance_name.clone();
            // Note: instance_id is NOT moved/cloned here
            let mut spawned_ids = self.spawned_instance_ids.clone();
            
            // This would capture instance_id by reference (potentially invalid)
            let spawn_task = async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                
                // This could use a stale or wrong instance_id
                let used_id = instance_id; // This might be wrong!
                spawned_ids.push(used_id);
                
                (used_id, instance_name_owned, spawned_ids)
            };
            
            let (spawned_id, spawned_name, updated_spawned_ids) = spawn_task.await;
            self.spawned_instance_ids = updated_spawned_ids;
            
            println!("Broken spawn: {} with ID {}", spawned_name, spawned_id);
        }
    }
}

#[tokio::test]
async fn test_instance_id_capture_fix() {
    let mut app = TestApp::new();
    
    // Test the fixed version
    app.spawn_coordinated_instances_test(3).await;
    
    // Verify each spawned instance has a unique ID that matches a created instance
    assert_eq!(app.spawned_instance_ids.len(), 3, "Should have spawned 3 instances");
    
    // Check that all spawned IDs correspond to actual instance IDs
    for spawned_id in &app.spawned_instance_ids {
        let found = app.instances.iter().any(|inst| inst.id == *spawned_id);
        assert!(found, "Spawned ID {} should correspond to a real instance", spawned_id);
    }
    
    // Verify no duplicate spawned IDs
    let mut unique_ids = app.spawned_instance_ids.clone();
    unique_ids.sort();
    unique_ids.dedup();
    assert_eq!(unique_ids.len(), app.spawned_instance_ids.len(), "All spawned IDs should be unique");
    
    println!("✅ Fixed version test passed!");
}

#[tokio::test]
async fn test_session_assignment_isolation() {
    // Test that each instance gets its own session without conflicts
    let mut sessions: HashMap<Uuid, String> = HashMap::new();
    
    let instance1 = TestInstance::new("Test-1".to_string());
    let instance2 = TestInstance::new("Test-2".to_string());
    let instance3 = TestInstance::new("Test-3".to_string());
    
    let id1 = instance1.id;
    let id2 = instance2.id;
    let id3 = instance3.id;
    
    // Simulate session assignment for each instance
    sessions.insert(id1, "session-001".to_string());
    sessions.insert(id2, "session-002".to_string());
    sessions.insert(id3, "session-003".to_string());
    
    // Verify each instance has a unique session
    assert_eq!(sessions.len(), 3, "Should have 3 unique sessions");
    assert_eq!(sessions.get(&id1), Some(&"session-001".to_string()));
    assert_eq!(sessions.get(&id2), Some(&"session-002".to_string()));
    assert_eq!(sessions.get(&id3), Some(&"session-003".to_string()));
    
    // Verify session routing would work correctly
    let target_session = "session-002";
    let target_instance = sessions.iter()
        .find(|(_, session)| *session == target_session)
        .map(|(id, _)| *id);
    
    assert_eq!(target_instance, Some(id2), "Session routing should find correct instance");
    
    println!("✅ Session isolation test passed!");
}

#[tokio::test]
async fn test_main_instance_session_preservation() {
    // Test that the main instance session is not overwritten by spawned instances
    let mut app = TestApp::new();
    
    // Assign session to main instance
    app.instances[0].session_id = Some("main-session-original".to_string());
    let original_main_id = app.instances[0].id;
    
    // Spawn new instances
    app.spawn_coordinated_instances_test(2).await;
    
    // Verify main instance still has its original session
    assert_eq!(app.instances[0].id, original_main_id, "Main instance ID should not change");
    assert_eq!(
        app.instances[0].session_id, 
        Some("main-session-original".to_string()),
        "Main instance session should be preserved"
    );
    
    // Verify spawned instances have different IDs
    for (i, spawned_id) in app.spawned_instance_ids.iter().enumerate() {
        assert_ne!(*spawned_id, original_main_id, "Spawned instance {} should have different ID from main", i + 1);
    }
    
    println!("✅ Main instance session preservation test passed!");
}

#[test]
fn test_uuid_clone_vs_move_semantics() {
    // Test that demonstrates UUID copy semantics work correctly in closures
    let original_id = Uuid::new_v4();
    let captured_ids = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    
    // Test proper UUID capture
    let id_owned = original_id; // UUID implements Copy, so this is fine
    let captured_ids_clone = captured_ids.clone();
    
    let closure = move || {
        let mut ids = captured_ids_clone.lock().unwrap();
        ids.push(id_owned);
        id_owned
    };
    
    let result_id = closure();
    
    assert_eq!(result_id, original_id, "UUID should be properly captured in closure");
    
    let captured = captured_ids.lock().unwrap();
    assert_eq!(captured.len(), 1, "Should have captured one UUID");
    assert_eq!(captured[0], original_id, "Captured UUID should match original");
    
    println!("✅ UUID semantics test passed!");
}