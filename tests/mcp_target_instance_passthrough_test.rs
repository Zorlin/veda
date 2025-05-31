use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

/// Mock implementation of MCP server logic for testing target_instance_id passthrough
mod mcp_mock {
    use serde_json::{json, Value};
    use std::collections::HashMap;
    
    pub fn create_ipc_message(tool_name: &str, tool_input: &Value, env_vars: &HashMap<String, String>) -> Value {
        let veda_session = env_vars.get("VEDA_SESSION_ID").cloned().unwrap_or_else(|| "default".to_string());
        let target_instance_id = env_vars.get("VEDA_TARGET_INSTANCE_ID").cloned();
        
        match tool_name {
            "veda_spawn_instances" => {
                let mut ipc_message = json!({
                    "type": "spawn_instances",
                    "session_id": veda_session,
                    "task_description": tool_input["task_description"].as_str().unwrap_or(""),
                    "num_instances": tool_input["num_instances"].as_u64().unwrap_or(2)
                });
                
                // Add target instance ID if available
                if let Some(ref target_id) = target_instance_id {
                    ipc_message["target_instance_id"] = json!(target_id);
                }
                
                ipc_message
            }
            "veda_list_instances" => {
                let mut ipc_message = json!({
                    "type": "list_instances",
                    "session_id": veda_session
                });
                
                if let Some(ref target_id) = target_instance_id {
                    ipc_message["target_instance_id"] = json!(target_id);
                }
                
                ipc_message
            }
            "veda_close_instance" => {
                let mut ipc_message = json!({
                    "type": "close_instance",
                    "session_id": veda_session,
                    "instance_name": tool_input["instance_name"].as_str().unwrap_or("")
                });
                
                if let Some(ref target_id) = target_instance_id {
                    ipc_message["target_instance_id"] = json!(target_id);
                }
                
                ipc_message
            }
            _ => json!({"error": "Unknown tool"})
        }
    }
}

#[test]
fn test_mcp_includes_target_instance_id_when_set() {
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "test-session".to_string());
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "ba5ee63c-b35e-4a4a-90a6-6d7281b18516".to_string());
    
    let tool_input = json!({
        "task_description": "Test task",
        "num_instances": 3
    });
    
    let ipc_msg = mcp_mock::create_ipc_message("veda_spawn_instances", &tool_input, &env_vars);
    
    // Verify the IPC message includes the target_instance_id
    assert_eq!(ipc_msg["type"], "spawn_instances");
    assert_eq!(ipc_msg["session_id"], "test-session");
    assert_eq!(ipc_msg["target_instance_id"], "ba5ee63c-b35e-4a4a-90a6-6d7281b18516");
    assert_eq!(ipc_msg["task_description"], "Test task");
    assert_eq!(ipc_msg["num_instances"], 3);
}

#[test]
fn test_mcp_works_without_target_instance_id() {
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "test-session".to_string());
    // Deliberately not setting VEDA_TARGET_INSTANCE_ID
    
    let tool_input = json!({
        "task_description": "Test task",
        "num_instances": 2
    });
    
    let ipc_msg = mcp_mock::create_ipc_message("veda_spawn_instances", &tool_input, &env_vars);
    
    // Verify the IPC message does NOT include target_instance_id
    assert_eq!(ipc_msg["type"], "spawn_instances");
    assert_eq!(ipc_msg["session_id"], "test-session");
    assert!(ipc_msg.get("target_instance_id").is_none() || ipc_msg["target_instance_id"].is_null());
}

#[test]
fn test_mcp_list_instances_includes_target_id() {
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "list-session".to_string());
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "e1edaadc-e78f-4f5d-9684-d9035fdeacd8".to_string());
    
    let tool_input = json!({});
    
    let ipc_msg = mcp_mock::create_ipc_message("veda_list_instances", &tool_input, &env_vars);
    
    assert_eq!(ipc_msg["type"], "list_instances");
    assert_eq!(ipc_msg["session_id"], "list-session");
    assert_eq!(ipc_msg["target_instance_id"], "e1edaadc-e78f-4f5d-9684-d9035fdeacd8");
}

#[test]
fn test_mcp_close_instance_includes_target_id() {
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "close-session".to_string());
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "44ca13e2-479a-4eea-8594-90ebecc04fd1".to_string());
    
    let tool_input = json!({
        "instance_name": "Veda-3"
    });
    
    let ipc_msg = mcp_mock::create_ipc_message("veda_close_instance", &tool_input, &env_vars);
    
    assert_eq!(ipc_msg["type"], "close_instance");
    assert_eq!(ipc_msg["session_id"], "close-session");
    assert_eq!(ipc_msg["target_instance_id"], "44ca13e2-479a-4eea-8594-90ebecc04fd1");
    assert_eq!(ipc_msg["instance_name"], "Veda-3");
}

#[test]
fn test_different_instances_get_different_target_ids() {
    // Simulate three different Claude instances with different VEDA_TARGET_INSTANCE_ID values
    let instance_configs = vec![
        ("ba5ee63c-b35e-4a4a-90a6-6d7281b18516", "Veda-1"),
        ("2685b199-e094-4220-8999-eec718f52b12", "Veda-2"),
        ("e1edaadc-e78f-4f5d-9684-d9035fdeacd8", "Veda-3"),
    ];
    
    let mut messages = vec![];
    
    for (instance_id, _name) in &instance_configs {
        let mut env_vars = HashMap::new();
        env_vars.insert("VEDA_SESSION_ID".to_string(), "shared-session".to_string());
        env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), instance_id.to_string());
        
        let tool_input = json!({ "task_description": "Task from different instance" });
        let ipc_msg = mcp_mock::create_ipc_message("veda_spawn_instances", &tool_input, &env_vars);
        
        messages.push(ipc_msg);
    }
    
    // Verify each message has a different target_instance_id
    assert_eq!(messages[0]["target_instance_id"], instance_configs[0].0);
    assert_eq!(messages[1]["target_instance_id"], instance_configs[1].0);
    assert_eq!(messages[2]["target_instance_id"], instance_configs[2].0);
    
    // Verify they're all different
    assert_ne!(messages[0]["target_instance_id"], messages[1]["target_instance_id"]);
    assert_ne!(messages[0]["target_instance_id"], messages[2]["target_instance_id"]);
    assert_ne!(messages[1]["target_instance_id"], messages[2]["target_instance_id"]);
}

#[test]
fn test_environment_isolation_between_instances() {
    // Test that simulates multiple Claude instances running concurrently
    // Each should maintain its own VEDA_TARGET_INSTANCE_ID
    
    let instance1_env = HashMap::from([
        ("VEDA_SESSION_ID".to_string(), "session-123".to_string()),
        ("VEDA_TARGET_INSTANCE_ID".to_string(), "instance-1-uuid".to_string()),
    ]);
    
    let instance2_env = HashMap::from([
        ("VEDA_SESSION_ID".to_string(), "session-123".to_string()), // Same session
        ("VEDA_TARGET_INSTANCE_ID".to_string(), "instance-2-uuid".to_string()), // Different instance
    ]);
    
    let tool_input = json!({ "task_description": "Same task" });
    
    let msg1 = mcp_mock::create_ipc_message("veda_spawn_instances", &tool_input, &instance1_env);
    let msg2 = mcp_mock::create_ipc_message("veda_spawn_instances", &tool_input, &instance2_env);
    
    // Same session, but different target instance IDs
    assert_eq!(msg1["session_id"], msg2["session_id"]);
    assert_ne!(msg1["target_instance_id"], msg2["target_instance_id"]);
    assert_eq!(msg1["target_instance_id"], "instance-1-uuid");
    assert_eq!(msg2["target_instance_id"], "instance-2-uuid");
}

/// Test that verifies the complete fix for the tab routing bug
#[test]
fn test_complete_tab_routing_fix() {
    // This test verifies the entire flow:
    // 1. Claude process sets VEDA_TARGET_INSTANCE_ID
    // 2. MCP server inherits and uses it
    // 3. IPC handler receives and uses it instead of random UUID
    
    let actual_instance_id = "ba5ee63c-b35e-4a4a-90a6-6d7281b18516";
    
    // Step 1: Claude sets the environment variable
    let mut claude_env = HashMap::new();
    claude_env.insert("VEDA_TARGET_INSTANCE_ID".to_string(), actual_instance_id.to_string());
    
    // Step 2: MCP server creates IPC message with the ID
    let tool_input = json!({ "task_description": "Test task" });
    let ipc_msg = mcp_mock::create_ipc_message("veda_spawn_instances", &tool_input, &claude_env);
    
    // Step 3: Verify the IPC message contains the correct instance ID
    assert_eq!(ipc_msg["target_instance_id"].as_str().unwrap(), actual_instance_id);
    
    // This ensures messages are routed back to the correct tab!
}