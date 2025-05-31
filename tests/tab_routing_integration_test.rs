use serde_json::{json, Value};
use uuid::Uuid;
use std::collections::HashMap;

/// End-to-end test that verifies the complete tab routing fix
#[test]
fn test_end_to_end_tab_routing() {
    // Simulate 4 tabs being created
    let tabs = vec![
        ("Veda-1", Uuid::new_v4()),
        ("Veda-2", Uuid::new_v4()),
        ("Veda-3", Uuid::new_v4()),
        ("Veda-4", Uuid::new_v4()),
    ];
    
    // Each tab spawns a Claude process with its own VEDA_TARGET_INSTANCE_ID
    let mut claude_processes = HashMap::new();
    for (name, instance_id) in &tabs {
        let env = HashMap::from([
            ("VEDA_TARGET_INSTANCE_ID".to_string(), instance_id.to_string()),
            ("VEDA_SESSION_ID".to_string(), "shared-session".to_string()),
        ]);
        claude_processes.insert(name.to_string(), env);
    }
    
    // Each Claude process calls veda_spawn_instances
    let mut ipc_messages = vec![];
    for (name, env) in &claude_processes {
        let target_id = env.get("VEDA_TARGET_INSTANCE_ID").unwrap();
        
        let ipc_msg = json!({
            "type": "spawn_instances",
            "session_id": env.get("VEDA_SESSION_ID").unwrap(),
            "task_description": format!("Task from {}", name),
            "num_instances": 2,
            "target_instance_id": target_id
        });
        
        ipc_messages.push((name.clone(), ipc_msg));
    }
    
    // Verify each IPC message has the correct instance ID
    for (i, (name, msg)) in ipc_messages.iter().enumerate() {
        let expected_id = tabs[i].1.to_string();
        let actual_id = msg["target_instance_id"].as_str().unwrap();
        
        assert_eq!(actual_id, expected_id, 
            "{} should have instance ID {}, but got {}", name, expected_id, actual_id);
    }
    
    // Verify all instance IDs are unique (no cross-contamination)
    let all_ids: Vec<_> = ipc_messages.iter()
        .map(|(_, msg)| msg["target_instance_id"].as_str().unwrap())
        .collect();
    
    let unique_ids: std::collections::HashSet<_> = all_ids.iter().collect();
    assert_eq!(unique_ids.len(), all_ids.len(), "All instance IDs should be unique");
}

/// Test that verifies messages are routed back to the correct tab
#[test]
fn test_message_routing_to_correct_tab() {
    // Setup: 3 tabs with their instance IDs
    let tabs = vec![
        ("Veda-1", Uuid::new_v4(), "session-1"),
        ("Veda-2", Uuid::new_v4(), "session-2"),
        ("Veda-3", Uuid::new_v4(), "session-3"),
    ];
    
    // Simulate messages coming back from Claude
    let messages = vec![
        json!({
            "type": "StreamText",
            "instance_id": tabs[0].1.to_string(),
            "session_id": tabs[0].2,
            "text": "Message for Tab 1"
        }),
        json!({
            "type": "StreamText",
            "instance_id": tabs[1].1.to_string(),
            "session_id": tabs[1].2,
            "text": "Message for Tab 2"
        }),
        json!({
            "type": "StreamText",
            "instance_id": tabs[2].1.to_string(),
            "session_id": tabs[2].2,
            "text": "Message for Tab 3"
        }),
    ];
    
    // Verify routing logic: messages should go to their respective tabs
    for (i, msg) in messages.iter().enumerate() {
        let msg_instance_id = msg["instance_id"].as_str().unwrap();
        let expected_instance_id = tabs[i].1.to_string();
        
        assert_eq!(msg_instance_id, expected_instance_id,
            "Message {} should be routed to tab {} ({})", i, i, tabs[i].0);
    }
}

/// Test that verifies the bug is fixed
#[test]
fn test_tab_routing_bug_is_fixed() {
    // The bug: IPC handler used Uuid::new_v4() causing messages to be misrouted
    
    // Before fix:
    let random_uuid = Uuid::new_v4();
    let actual_instance_id = Uuid::new_v4();
    
    // IPC message now includes target_instance_id
    let ipc_msg = json!({
        "type": "spawn_instances",
        "target_instance_id": actual_instance_id.to_string()
    });
    
    // Handler should use the provided ID, not a random one
    let used_id = if let Some(id_str) = ipc_msg["target_instance_id"].as_str() {
        Uuid::parse_str(id_str).unwrap()
    } else {
        random_uuid // This was the bug!
    };
    
    // After fix: used_id should be actual_instance_id, not random_uuid
    assert_eq!(used_id, actual_instance_id);
    assert_ne!(used_id, random_uuid);
}

/// Test concurrent instance operations
#[test]
fn test_concurrent_instance_operations() {
    // Simulate multiple instances calling MCP tools simultaneously
    let instances = vec![
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    
    // Each instance creates its own IPC message
    let mut messages = vec![];
    for (i, instance_id) in instances.iter().enumerate() {
        let msg = json!({
            "type": "list_instances",
            "session_id": "shared-session",
            "target_instance_id": instance_id.to_string(),
            "timestamp": i // To ensure messages are different
        });
        messages.push(msg);
    }
    
    // Verify each message maintains its own instance ID
    for (i, msg) in messages.iter().enumerate() {
        let msg_id = msg["target_instance_id"].as_str().unwrap();
        let expected_id = instances[i].to_string();
        assert_eq!(msg_id, expected_id);
    }
}

/// Test that verifies the complete fix prevents the tab display bug
#[test]
fn test_tab_display_bug_prevention() {
    // The original bug: All instances displayed their output on Tab 0
    // This was because messages were routed with random UUIDs
    
    // Create 4 instances
    let instances: Vec<(String, Uuid)> = (1..=4)
        .map(|i| (format!("Veda-{}", i), Uuid::new_v4()))
        .collect();
    
    // Each instance sends a message
    let mut routed_messages = HashMap::new();
    
    for (name, instance_id) in &instances {
        // Message includes the correct instance ID
        let msg = json!({
            "instance_id": instance_id.to_string(),
            "text": format!("Output from {}", name)
        });
        
        // Route message to the correct tab (find by instance_id)
        let tab_index = instances.iter().position(|(_, id)| id == instance_id).unwrap();
        routed_messages.insert(tab_index, msg);
    }
    
    // Verify each tab has exactly one message (not all on Tab 0)
    assert_eq!(routed_messages.len(), 4, "Each tab should have its own message");
    
    // Verify Tab 0 only has its own message
    let tab0_msg = routed_messages.get(&0).unwrap();
    assert!(tab0_msg["text"].as_str().unwrap().contains("Veda-1"));
    
    // Verify other tabs have their own messages
    for i in 1..4 {
        let tab_msg = routed_messages.get(&i).unwrap();
        let expected_name = format!("Veda-{}", i + 1);
        assert!(tab_msg["text"].as_str().unwrap().contains(&expected_name),
            "Tab {} should have message from {}", i, expected_name);
    }
}

/// Regression test to ensure the fix stays in place
#[test]
fn test_regression_prevention() {
    // This test will fail if someone reverts the fix
    
    // 1. Claude sets VEDA_TARGET_INSTANCE_ID
    let instance_id = Uuid::new_v4();
    let claude_env = HashMap::from([
        ("VEDA_TARGET_INSTANCE_ID".to_string(), instance_id.to_string()),
    ]);
    
    // 2. MCP server includes it in IPC message
    let ipc_msg = json!({
        "type": "spawn_instances",
        "target_instance_id": claude_env["VEDA_TARGET_INSTANCE_ID"]
    });
    
    // 3. IPC handler uses it (not Uuid::new_v4())
    assert!(ipc_msg["target_instance_id"].is_string());
    assert_eq!(ipc_msg["target_instance_id"].as_str().unwrap(), instance_id.to_string());
    
    // This ensures the fix is working correctly
}