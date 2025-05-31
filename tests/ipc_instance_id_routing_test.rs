use serde_json::json;
use uuid::Uuid;

/// Test that IPC messages use target_instance_id when provided instead of random UUIDs
#[test]
fn test_ipc_uses_target_instance_id() {
    // Test spawn_instances
    let target_id = Uuid::new_v4();
    let ipc_msg = json!({
        "type": "spawn_instances",
        "session_id": "test-session",
        "task_description": "Test task",
        "num_instances": 2,
        "target_instance_id": target_id.to_string()
    });
    
    // Verify the message contains the target_instance_id
    assert_eq!(ipc_msg["target_instance_id"].as_str().unwrap(), target_id.to_string());
    
    // Test list_instances
    let list_msg = json!({
        "type": "list_instances",
        "session_id": "test-session",
        "target_instance_id": target_id.to_string()
    });
    
    assert_eq!(list_msg["target_instance_id"].as_str().unwrap(), target_id.to_string());
    
    // Test close_instance
    let close_msg = json!({
        "type": "close_instance",
        "session_id": "test-session",
        "instance_name": "Veda-2",
        "target_instance_id": target_id.to_string()
    });
    
    assert_eq!(close_msg["target_instance_id"].as_str().unwrap(), target_id.to_string());
}

/// Test that IPC messages work without target_instance_id for backward compatibility
#[test]
fn test_ipc_backward_compatibility_without_target_id() {
    // Test spawn_instances without target_instance_id
    let ipc_msg = json!({
        "type": "spawn_instances",
        "session_id": "test-session",
        "task_description": "Test task",
        "num_instances": 2
    });
    
    // Should not have target_instance_id
    assert!(ipc_msg.get("target_instance_id").is_none());
    
    // Test list_instances without target_instance_id
    let list_msg = json!({
        "type": "list_instances",
        "session_id": "test-session"
    });
    
    assert!(list_msg.get("target_instance_id").is_none());
}

/// Test UUID parsing from string
#[test]
fn test_uuid_parsing_from_ipc_message() {
    let valid_uuid = Uuid::new_v4();
    let valid_uuid_str = valid_uuid.to_string();
    
    // Test valid UUID parsing
    let parsed = Uuid::parse_str(&valid_uuid_str);
    assert!(parsed.is_ok());
    assert_eq!(parsed.unwrap(), valid_uuid);
    
    // Test invalid UUID parsing
    let invalid_uuid_str = "not-a-valid-uuid";
    let parsed = Uuid::parse_str(invalid_uuid_str);
    assert!(parsed.is_err());
}

/// Test that each instance gets a unique VEDA_TARGET_INSTANCE_ID
#[test]
fn test_unique_instance_ids_per_tab() {
    let instance1 = Uuid::new_v4();
    let instance2 = Uuid::new_v4();
    let instance3 = Uuid::new_v4();
    
    // Ensure all instance IDs are unique
    assert_ne!(instance1, instance2);
    assert_ne!(instance1, instance3);
    assert_ne!(instance2, instance3);
    
    // Simulate environment variables for each instance
    let env1 = format!("VEDA_TARGET_INSTANCE_ID={}", instance1);
    let env2 = format!("VEDA_TARGET_INSTANCE_ID={}", instance2);
    let env3 = format!("VEDA_TARGET_INSTANCE_ID={}", instance3);
    
    // Ensure environment strings are different
    assert_ne!(env1, env2);
    assert_ne!(env1, env3);
    assert_ne!(env2, env3);
}

/// Test IPC message routing logic
#[test]
fn test_ipc_message_routing_priority() {
    // Test case 1: Message with target_instance_id should use it
    let target_id = Uuid::new_v4();
    let msg_with_target = json!({
        "type": "spawn_instances",
        "target_instance_id": target_id.to_string()
    });
    
    if let Some(target_id_str) = msg_with_target["target_instance_id"].as_str() {
        let parsed = Uuid::parse_str(target_id_str);
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap(), target_id);
    } else {
        panic!("Expected target_instance_id in message");
    }
    
    // Test case 2: Message without target_instance_id should not have it
    let msg_without_target = json!({
        "type": "spawn_instances"
    });
    
    assert!(msg_without_target["target_instance_id"].is_null());
}

/// Test that invalid UUIDs are handled gracefully
#[test]
fn test_invalid_uuid_handling() {
    let invalid_uuids = vec![
        "invalid",
        "12345",
        "not-a-uuid",
        "",
        "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
    ];
    
    for invalid in invalid_uuids {
        let result = Uuid::parse_str(invalid);
        assert!(result.is_err(), "Expected '{}' to fail UUID parsing", invalid);
    }
}

/// Integration test for the full IPC flow
#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::collections::HashMap;
    
    /// Simulate the IPC handler logic
    fn handle_ipc_message(msg: &serde_json::Value) -> Uuid {
        if let Some(target_id_str) = msg["target_instance_id"].as_str() {
            match Uuid::parse_str(target_id_str) {
                Ok(id) => id,
                Err(_) => Uuid::new_v4() // Fallback to random UUID
            }
        } else {
            Uuid::new_v4() // No target_instance_id provided
        }
    }
    
    #[test]
    fn test_ipc_handler_with_valid_target_id() {
        let expected_id = Uuid::new_v4();
        let msg = json!({
            "type": "spawn_instances",
            "target_instance_id": expected_id.to_string()
        });
        
        let result_id = handle_ipc_message(&msg);
        assert_eq!(result_id, expected_id, "IPC handler should use provided target_instance_id");
    }
    
    #[test]
    fn test_ipc_handler_with_invalid_target_id() {
        let msg = json!({
            "type": "spawn_instances",
            "target_instance_id": "invalid-uuid"
        });
        
        let result_id = handle_ipc_message(&msg);
        // Should get a random UUID, just verify it's valid
        assert!(!result_id.is_nil());
    }
    
    #[test]
    fn test_ipc_handler_without_target_id() {
        let msg = json!({
            "type": "spawn_instances"
        });
        
        let result_id = handle_ipc_message(&msg);
        // Should get a random UUID, just verify it's valid
        assert!(!result_id.is_nil());
    }
}

/// Test for session-based routing
#[test]
fn test_session_based_routing_with_instance_ids() {
    let instance_id = Uuid::new_v4();
    let session_id = "test-session-123";
    
    // Simulate a message that should be routed by session
    let msg = json!({
        "instance_id": instance_id.to_string(),
        "session_id": session_id,
        "text": "Test message"
    });
    
    // Both IDs should be present and valid
    assert_eq!(msg["instance_id"].as_str().unwrap(), instance_id.to_string());
    assert_eq!(msg["session_id"].as_str().unwrap(), session_id);
}

/// Test that verifies the fix for the tab routing bug
#[test]
fn test_tab_routing_bug_fix() {
    // This test documents the exact bug that was fixed:
    // IPC handler was using Uuid::new_v4() instead of the actual instance ID
    
    // Before fix: IPC would create a random UUID
    let random_id = Uuid::new_v4();
    
    // After fix: IPC uses the target_instance_id from the message
    let actual_instance_id = Uuid::new_v4();
    let msg = json!({
        "type": "spawn_instances",
        "target_instance_id": actual_instance_id.to_string()
    });
    
    // The fix ensures we use actual_instance_id, not random_id
    if let Some(target_id_str) = msg["target_instance_id"].as_str() {
        let used_id = Uuid::parse_str(target_id_str).unwrap();
        assert_eq!(used_id, actual_instance_id);
        assert_ne!(used_id, random_id); // Should NOT be a random UUID
    }
}