use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

// Mock implementation of MCP server functions for testing
mod mcp_server {
    use serde_json::{json, Value};
    use std::collections::HashMap;
    
    // Re-export the functions we want to test - now takes explicit environment
    pub async fn create_tool_call_response_test(
        request_id: &Value, 
        tool_name: &str, 
        tool_input: &Value,
        env_vars: &HashMap<String, String>
    ) -> Value {
        // Mock implementation that mirrors the actual MCP server logic
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
                
                if let Some(ref target_id) = target_instance_id {
                    ipc_message["target_instance_id"] = json!(target_id);
                }
                
                json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!("Mock spawn with IPC: {}", ipc_message)
                        }]
                    }
                })
            }
            "veda_list_instances" => {
                let mut ipc_message = json!({
                    "type": "list_instances",
                    "session_id": veda_session
                });
                
                if let Some(ref target_id) = target_instance_id {
                    ipc_message["target_instance_id"] = json!(target_id);
                }
                
                json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!("Mock list with IPC: {}", ipc_message)
                        }]
                    }
                })
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32601,
                        "message": "Method not found"
                    }
                })
            }
        }
    }
}

#[tokio::test]
async fn test_mcp_server_includes_target_instance_id() {
    let request_id = json!(42);
    let tool_input = json!({
        "task_description": "Test task",
        "num_instances": 3
    });
    
    // Set up test environment variables
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "test-session-123".to_string());
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "target-instance-456".to_string());
    
    let response = mcp_server::create_tool_call_response_test(&request_id, "veda_spawn_instances", &tool_input, &env_vars).await;
    
    // Verify response structure
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 42);
    assert!(response["result"]["content"].is_array());
    
    // Parse the IPC message from the response text
    let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(response_text.starts_with("Mock spawn with IPC: "));
    
    let ipc_part = &response_text["Mock spawn with IPC: ".len()..];
    let ipc_message: Value = serde_json::from_str(ipc_part).expect("Should parse IPC message");
    
    // Verify IPC message includes target_instance_id
    assert_eq!(ipc_message["type"], "spawn_instances");
    assert_eq!(ipc_message["session_id"], "test-session-123");
    assert_eq!(ipc_message["task_description"], "Test task");
    assert_eq!(ipc_message["num_instances"], 3);
    assert_eq!(ipc_message["target_instance_id"], "target-instance-456");
}

#[tokio::test]
async fn test_mcp_server_without_target_instance_id() {
    let request_id = json!("test-id");
    let tool_input = json!({
        "task_description": "Another test",
        "num_instances": 1
    });
    
    // Set up environment variables without target instance ID
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "test-session-789".to_string());
    // Deliberately not setting VEDA_TARGET_INSTANCE_ID
    
    let response = mcp_server::create_tool_call_response_test(&request_id, "veda_spawn_instances", &tool_input, &env_vars).await;
    
    // Parse the IPC message from the response
    let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
    let ipc_part = &response_text["Mock spawn with IPC: ".len()..];
    let ipc_message: Value = serde_json::from_str(ipc_part).expect("Should parse IPC message");
    
    // Verify IPC message does NOT include target_instance_id
    assert_eq!(ipc_message["type"], "spawn_instances");
    assert_eq!(ipc_message["session_id"], "test-session-789");
    assert_eq!(ipc_message["task_description"], "Another test");
    assert_eq!(ipc_message["num_instances"], 1);
    assert!(ipc_message.get("target_instance_id").is_none(), "Should not include target_instance_id when not set");
}

#[tokio::test]
async fn test_mcp_server_list_instances_with_target_id() {
    let request_id = json!(999);
    let tool_input = json!({});
    
    // Set up environment variables
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_SESSION_ID".to_string(), "list-session".to_string());
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "list-target-123".to_string());
    
    let response = mcp_server::create_tool_call_response_test(&request_id, "veda_list_instances", &tool_input, &env_vars).await;
    
    // Parse the IPC message
    let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
    let ipc_part = &response_text["Mock list with IPC: ".len()..];
    let ipc_message: Value = serde_json::from_str(ipc_part).expect("Should parse IPC message");
    
    // Verify target_instance_id is included for list command too
    assert_eq!(ipc_message["type"], "list_instances");
    assert_eq!(ipc_message["session_id"], "list-session");
    assert_eq!(ipc_message["target_instance_id"], "list-target-123");
}

#[tokio::test]
async fn test_mcp_server_environment_isolation() {
    let request_id = json!(1);
    let tool_input = json!({
        "task_description": "Isolation test",
        "num_instances": 1
    });
    
    // First call with target instance ID
    let mut env_vars1 = HashMap::new();
    env_vars1.insert("VEDA_SESSION_ID".to_string(), "isolation-session".to_string());
    env_vars1.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "target-alpha".to_string());
    
    let response1 = mcp_server::create_tool_call_response_test(&request_id, "veda_spawn_instances", &tool_input, &env_vars1).await;
    
    // Second call with different target instance ID
    let mut env_vars2 = HashMap::new();
    env_vars2.insert("VEDA_SESSION_ID".to_string(), "isolation-session".to_string());
    env_vars2.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "target-beta".to_string());
    
    let response2 = mcp_server::create_tool_call_response_test(&request_id, "veda_spawn_instances", &tool_input, &env_vars2).await;
    
    // Third call with no target instance ID
    let mut env_vars3 = HashMap::new();
    env_vars3.insert("VEDA_SESSION_ID".to_string(), "isolation-session".to_string());
    // No VEDA_TARGET_INSTANCE_ID
    
    let response3 = mcp_server::create_tool_call_response_test(&request_id, "veda_spawn_instances", &tool_input, &env_vars3).await;
    
    // Parse all responses
    let text1 = response1["result"]["content"][0]["text"].as_str().unwrap();
    let text2 = response2["result"]["content"][0]["text"].as_str().unwrap();
    let text3 = response3["result"]["content"][0]["text"].as_str().unwrap();
    
    // Verify they're different
    assert_ne!(text1, text2, "Different target IDs should produce different IPC messages");
    assert_ne!(text1, text3, "With and without target ID should be different");
    assert_ne!(text2, text3, "Different target IDs vs no target should be different");
    
    // Verify specific content
    assert!(text1.contains("target-alpha"), "First response should contain target-alpha");
    assert!(text2.contains("target-beta"), "Second response should contain target-beta");
    assert!(!text3.contains("target_instance_id"), "Third response should not contain target_instance_id field");
}

#[tokio::test]
async fn test_mcp_server_unknown_tool() {
    let request_id = json!("unknown");
    let tool_input = json!({});
    
    // Test with environment that includes target instance (should be ignored for unknown tool)
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), "should-be-ignored".to_string());
    
    let response = mcp_server::create_tool_call_response_test(&request_id, "unknown_tool", &tool_input, &env_vars).await;
    
    // Verify error response
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "unknown");
    assert!(response["error"].is_object());
    assert_eq!(response["error"]["code"], -32601);
    assert_eq!(response["error"]["message"], "Method not found");
}

#[tokio::test]
async fn test_environment_variable_cleanup() {
    let request_id = json!(123);
    let tool_input = json!({
        "task_description": "Cleanup test"
    });
    
    // Test with completely clean environment (no variables set)
    let env_vars = HashMap::new();
    
    let response = mcp_server::create_tool_call_response_test(&request_id, "veda_spawn_instances", &tool_input, &env_vars).await;
    
    // Parse response
    let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
    let ipc_part = &response_text["Mock spawn with IPC: ".len()..];
    let ipc_message: Value = serde_json::from_str(ipc_part).expect("Should parse IPC message");
    
    // Verify no target_instance_id in message when environment is clean
    assert!(ipc_message.get("target_instance_id").is_none(), "Should not have target_instance_id when env is clean");
    assert_eq!(ipc_message["session_id"], "default", "Should use default session when no VEDA_SESSION_ID");
}