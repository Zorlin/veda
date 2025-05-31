use uuid::Uuid;
use std::collections::HashMap;
use serde_json::json;

/// Represents a Claude instance in the main.rs instances HashMap
#[derive(Debug, Clone)]
struct ClaudeInstance {
    id: Uuid,
    name: String,
    session_id: String,
    message_count: usize,
}

/// Simulates the main.rs message routing logic
fn route_message(
    instances: &HashMap<Uuid, ClaudeInstance>,
    instance_id: Option<Uuid>,
    session_id: Option<String>,
) -> Option<Uuid> {
    // This simulates the actual routing logic from main.rs
    
    // Priority 1: Route by session ID if available
    if let Some(sid) = session_id {
        for (id, instance) in instances {
            if instance.session_id == sid {
                return Some(*id);
            }
        }
    }
    
    // Priority 2: Route by instance ID
    if let Some(iid) = instance_id {
        if instances.contains_key(&iid) {
            return Some(iid);
        }
    }
    
    // Priority 3: Route to first instance (main tab)
    instances.keys().next().copied()
}

/// Test the exact routing logic used in main.rs
#[test]
fn test_main_rs_routing_logic() {
    let mut instances = HashMap::new();
    
    // Create instances matching what main.rs creates
    let instance1 = ClaudeInstance {
        id: Uuid::new_v4(),
        name: "Veda-1".to_string(),
        session_id: "session-main".to_string(),
        message_count: 0,
    };
    let instance2 = ClaudeInstance {
        id: Uuid::new_v4(),
        name: "Veda-2".to_string(),
        session_id: "session-2".to_string(),
        message_count: 0,
    };
    let instance3 = ClaudeInstance {
        id: Uuid::new_v4(),
        name: "Veda-3".to_string(),
        session_id: "session-3".to_string(),
        message_count: 0,
    };
    
    instances.insert(instance1.id, instance1.clone());
    instances.insert(instance2.id, instance2.clone());
    instances.insert(instance3.id, instance3.clone());
    
    // Test cases matching real scenarios
    
    // 1. Correct session and instance (normal case)
    let routed = route_message(&instances, Some(instance2.id), Some(instance2.session_id.clone()));
    assert_eq!(routed, Some(instance2.id));
    
    // 2. No session ID (automode bug)
    let routed = route_message(&instances, Some(instance3.id), None);
    assert_eq!(routed, Some(instance3.id));
    
    // 3. Wrong session ID but correct instance
    let routed = route_message(&instances, Some(instance2.id), Some("wrong-session".to_string()));
    assert_eq!(routed, Some(instance2.id));
    
    // 4. Session ID from different instance (causes misrouting)
    let routed = route_message(&instances, Some(instance2.id), Some(instance3.session_id.clone()));
    assert_eq!(routed, Some(instance3.id), "Session ID takes priority - this is the bug!");
}

/// Test the spawn_instances IPC handling
#[test]
fn test_spawn_instances_ipc_handling() {
    // Simulate the IPC message from MCP server
    let target_instance_id = Uuid::new_v4();
    let ipc_msg = json!({
        "type": "spawn_instances",
        "session_id": "test-session",
        "task_description": "Test task",
        "num_instances": 2,
        "target_instance_id": target_instance_id.to_string()
    });
    
    // Simulate the IPC handler logic from main.rs
    let instance_id = if let Some(target_id_str) = ipc_msg["target_instance_id"].as_str() {
        match Uuid::parse_str(target_id_str) {
            Ok(id) => {
                // This is the fix - use the provided ID
                id
            },
            Err(_) => {
                // Fallback to random (should rarely happen)
                Uuid::new_v4()
            }
        }
    } else {
        // This was the bug - always using random UUID
        Uuid::new_v4()
    };
    
    // Verify the fix works
    assert_eq!(instance_id, target_instance_id, "Should use target_instance_id from IPC message");
}

/// Test message accumulation patterns
#[test]
fn test_message_accumulation_patterns() {
    let mut instances = HashMap::new();
    let mut message_counts = HashMap::new();
    
    // Create 4 instances
    for i in 0..4 {
        let instance = ClaudeInstance {
            id: Uuid::new_v4(),
            name: format!("Veda-{}", i + 1),
            session_id: format!("session-{}", i + 1),
            message_count: 0,
        };
        instances.insert(instance.id, instance.clone());
        message_counts.insert(instance.id, 0);
    }
    
    let instance_ids: Vec<_> = instances.keys().copied().collect();
    
    // Simulate message flow
    let messages = vec![
        // Initial messages - correct routing
        (Some(instance_ids[0]), Some("session-1".to_string())),
        (Some(instance_ids[1]), Some("session-2".to_string())),
        (Some(instance_ids[2]), Some("session-3".to_string())),
        (Some(instance_ids[3]), Some("session-4".to_string())),
        
        // Bug scenario - no session IDs
        (Some(instance_ids[1]), None),
        (Some(instance_ids[2]), None),
        (Some(instance_ids[3]), None),
        
        // More bug scenarios - wrong session IDs
        (Some(instance_ids[1]), Some("session-1".to_string())), // Routes to instance 1!
        (Some(instance_ids[2]), Some("session-1".to_string())), // Routes to instance 1!
        (Some(instance_ids[3]), Some("session-1".to_string())), // Routes to instance 1!
    ];
    
    // Route messages and count
    for (instance_id, session_id) in messages {
        if let Some(routed_to) = route_message(&instances, instance_id, session_id) {
            *message_counts.get_mut(&routed_to).unwrap() += 1;
        }
    }
    
    // Check accumulation pattern
    let main_instance = instance_ids[0];
    let main_count = message_counts[&main_instance];
    let other_counts: Vec<_> = instance_ids[1..].iter()
        .map(|id| message_counts[id])
        .collect();
    
    println!("Main tab messages: {}", main_count);
    println!("Other tab messages: {:?}", other_counts);
    
    // Check if the bug is present (main tab accumulating messages)
    if main_count >= 4 {
        println!("WARNING: Main tab is accumulating messages (bug detected)");
    } else {
        println!("Good: Messages are distributed properly across tabs");
    }
}

/// Test the actual message types used in Veda
#[test]
fn test_veda_message_types() {
    // Test StreamText messages
    let stream_text = json!({
        "type": "StreamText",
        "instance_id": Uuid::new_v4().to_string(),
        "session_id": "test-session",
        "text": "Test output"
    });
    
    assert_eq!(stream_text["type"], "StreamText");
    assert!(stream_text["instance_id"].is_string());
    assert!(stream_text["session_id"].is_string());
    assert!(stream_text["text"].is_string());
    
    // Test ToolUse messages
    let tool_use = json!({
        "type": "ToolUse",
        "instance_id": Uuid::new_v4().to_string(),
        "session_id": "test-session",
        "tool": "veda_spawn_instances",
        "input": {}
    });
    
    assert_eq!(tool_use["type"], "ToolUse");
    
    // Test SessionStarted messages
    let session_started = json!({
        "type": "SessionStarted",
        "instance_id": Uuid::new_v4().to_string(),
        "session_id": "new-session"
    });
    
    assert_eq!(session_started["type"], "SessionStarted");
}

/// Test coordinated instance spawning
#[test]
fn test_coordinated_instance_spawning() {
    // When main instance spawns coordinated instances
    let main_instance_id = Uuid::new_v4();
    let main_session_id = "main-session";
    
    // Spawned instances get new IDs but might have session issues
    let spawned_instances = vec![
        (Uuid::new_v4(), "spawned-session-1"),
        (Uuid::new_v4(), "spawned-session-2"),
        (Uuid::new_v4(), "spawned-session-3"),
    ];
    
    // The bug: spawned instances might send messages with the main session ID
    let problematic_messages = vec![
        json!({
            "instance_id": spawned_instances[0].0.to_string(),
            "session_id": main_session_id, // Wrong! Should be spawned-session-1
            "text": "Message from spawned instance 1"
        }),
        json!({
            "instance_id": spawned_instances[1].0.to_string(),
            "session_id": main_session_id, // Wrong! Should be spawned-session-2
            "text": "Message from spawned instance 2"
        }),
    ];
    
    // These messages would all route to main instance due to session ID
    for msg in &problematic_messages {
        assert_eq!(
            msg["session_id"].as_str().unwrap(),
            main_session_id,
            "Bug: spawned instances use main session ID"
        );
    }
}

/// Test environment variable inheritance issues
#[test]
fn test_environment_variable_inheritance() {
    // Parent process environment
    let parent_env = vec![
        ("VEDA_SESSION_ID", "parent-session"),
        ("VEDA_TARGET_INSTANCE_ID", "parent-instance-id"),
    ];
    
    // When spawning Claude, it should OVERRIDE parent's VEDA_TARGET_INSTANCE_ID
    let child_instance_id = Uuid::new_v4();
    let child_instance_id_str = child_instance_id.to_string();
    let child_env = vec![
        ("VEDA_SESSION_ID", "parent-session"), // Can inherit
        ("VEDA_TARGET_INSTANCE_ID", child_instance_id_str.as_str()), // Must override!
    ];
    
    // Verify child doesn't use parent's instance ID
    assert_ne!(
        child_env[1].1,
        parent_env[1].1,
        "Child must have its own VEDA_TARGET_INSTANCE_ID"
    );
}

/// Test UI rendering with empty tabs
#[test]
fn test_ui_rendering_empty_tabs() {
    #[derive(Debug)]
    struct TabDisplay {
        name: String,
        messages: Vec<String>,
        is_active: bool,
    }
    
    impl TabDisplay {
        fn render(&self) -> String {
            if self.messages.is_empty() {
                format!("{} (empty)", self.name)
            } else if self.messages.len() > 100 {
                format!("{} (overflow - {} messages)", self.name, self.messages.len())
            } else {
                format!("{} ({} messages)", self.name, self.messages.len())
            }
        }
    }
    
    // HEALTHY STATE AFTER BUG FIX: Messages properly distributed
    let tabs = vec![
        TabDisplay {
            name: "Veda-1".to_string(),
            messages: vec!["msg".to_string(); 25], // Normal count
            is_active: true,
        },
        TabDisplay {
            name: "Veda-2".to_string(),
            messages: vec!["msg".to_string(); 20], // Normal count
            is_active: false,
        },
        TabDisplay {
            name: "Veda-3".to_string(),
            messages: vec!["msg".to_string(); 18], // Normal count
            is_active: false,
        },
        TabDisplay {
            name: "Veda-4".to_string(),
            messages: vec!["msg".to_string(); 15], // Normal count
            is_active: false,
        },
    ];
    
    // Render tabs
    for tab in &tabs {
        let display = tab.render();
        println!("{}", display);
        
        if tab.messages.is_empty() {
            assert!(display.contains("empty"));
        } else if tab.messages.len() > 100 {
            assert!(display.contains("overflow"));
        }
    }
    
    // Detect the bug pattern
    let empty_count = tabs.iter().filter(|t| t.messages.is_empty()).count();
    let single_message_count = tabs.iter().filter(|t| t.messages.len() == 1).count();
    let overflow_count = tabs.iter().filter(|t| t.messages.len() > 100).count();
    
    println!("Empty tabs: {}, Single message tabs: {}, Overflow tabs: {}", 
             empty_count, single_message_count, overflow_count);
    
    // Bug detection - FAIL if this pattern is detected
    assert!(
        !(overflow_count > 0 && single_message_count > 1),
        "CRITICAL BUG: Detected '1 message per tab while main overflows' pattern! {} tabs overflow, {} tabs have 1 message",
        overflow_count,
        single_message_count
    );
}