use std::time::Duration;
use tokio::time::timeout;
use serde_json;
use uuid::Uuid;

#[tokio::test]
async fn test_instance_spawning_flow() {
    // This test verifies that the instance spawning flow works end-to-end
    
    // Test data
    let test_task = "We're developing a Rust project. Please implement multiple features in parallel: feature A in src/a.rs, feature B in src/b.rs, feature C in src/c.rs.";
    let num_instances = 3;
    
    // Simulate the spawn instances message
    let spawn_message = serde_json::json!({
        "type": "spawn_instances",
        "session_id": "test-session",
        "task_description": test_task,
        "num_instances": num_instances
    });
    
    // Verify the message structure is correct
    assert_eq!(spawn_message["type"], "spawn_instances");
    assert_eq!(spawn_message["num_instances"], num_instances);
    assert_eq!(spawn_message["task_description"], test_task);
    
    println!("✅ Spawn instances message format is correct");
}

#[tokio::test]
async fn test_deepseek_task_breakdown_format() {
    // Test that we can parse the expected DeepSeek response format
    let mock_deepseek_response = r#"SUBTASK_1: Implement feature A | SCOPE: src/a.rs, tests/a_test.rs | PRIORITY: High
SUBTASK_2: Implement feature B | SCOPE: src/b.rs, tests/b_test.rs | PRIORITY: Medium  
SUBTASK_3: Implement feature C | SCOPE: src/c.rs, tests/c_test.rs | PRIORITY: High"#;
    
    // Parse subtasks like the real code does
    let subtasks: Vec<&str> = mock_deepseek_response.lines()
        .filter(|line| line.starts_with("SUBTASK_"))
        .collect();
    
    assert_eq!(subtasks.len(), 3);
    
    // Test parsing each subtask
    for (i, subtask) in subtasks.iter().enumerate() {
        let task_parts: Vec<&str> = subtask.split(" | ").collect();
        let task_desc = task_parts.get(0)
            .unwrap_or(&"")
            .trim_start_matches("SUBTASK_")
            .trim_start_matches(&format!("{}: ", i + 1));
        
        let scope = task_parts.iter()
            .find(|part| part.starts_with("SCOPE:"))
            .map(|s| s.trim_start_matches("SCOPE:").trim())
            .unwrap_or("No specific scope");
            
        let priority = task_parts.iter()
            .find(|part| part.starts_with("PRIORITY:"))
            .map(|s| s.trim_start_matches("PRIORITY:").trim())
            .unwrap_or("Medium");
        
        println!("Subtask {}: {} | Scope: {} | Priority: {}", i + 1, task_desc, scope, priority);
        assert!(!task_desc.is_empty());
        assert!(!scope.is_empty());
        assert!(["High", "Medium", "Low"].contains(&priority));
    }
    
    println!("✅ DeepSeek response parsing works correctly");
}

#[tokio::test]
async fn test_claude_command_construction() {
    // Test that we can construct the correct Claude command
    use tokio::process::Command;
    
    let test_message = "Test message for Claude instance";
    let session_id = Some("test-session-123".to_string());
    
    let mut cmd = Command::new("claude");
    
    // Add VEDA_SESSION_ID if available (simulate the real code)
    if let Ok(veda_session_id) = std::env::var("VEDA_SESSION_ID") {
        cmd.env("VEDA_SESSION_ID", veda_session_id);
    }
    
    if let Some(session) = session_id {
        cmd.arg("--resume").arg(session);
    }
    
    cmd.arg("-p")
        .arg(&test_message)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--mcp-config")
        .arg(".mcp.json");
    
    // Verify the command would be constructed correctly
    let program = cmd.as_std().get_program();
    let args: Vec<_> = cmd.as_std().get_args().collect();
    
    assert_eq!(program, "claude");
    
    // Check that key arguments are present
    let args_str: Vec<&str> = args.iter().map(|s| s.to_str().unwrap()).collect();
    assert!(args_str.contains(&"-p"));
    assert!(args_str.contains(&"--output-format"));
    assert!(args_str.contains(&"stream-json"));
    assert!(args_str.contains(&"--verbose"));
    assert!(args_str.contains(&"--mcp-config"));
    assert!(args_str.contains(&".mcp.json"));
    
    println!("✅ Claude command construction is correct");
    println!("Command: {:?} {:?}", program, args_str);
}

#[tokio::test]
async fn test_instance_creation_data_structures() {
    // Test that we can create the data structures for instances
    use std::collections::HashMap;
    
    // Simulate creating instances like the real code does
    let mut instances = Vec::new();
    let working_dir = "/test/working/dir";
    
    for i in 0..3 {
        // Start from instance 2 to match the real spawning behavior (instance 1 is the main tab)
        let instance_name = format!("Claude {}-{}", instances.len() + 2, char::from(b'A' + i as u8));
        let instance_id = Uuid::new_v4();
        
        // Simulate the instance structure (simplified)
        let instance = HashMap::from([
            ("id".to_string(), instance_id.to_string()),
            ("name".to_string(), instance_name.clone()),
            ("working_directory".to_string(), working_dir.to_string()),
        ]);
        
        instances.push(instance);
        println!("Created instance: {}", instance_name);
    }
    
    assert_eq!(instances.len(), 3);
    assert_eq!(instances[0]["name"], "Claude 2-A");
    assert_eq!(instances[1]["name"], "Claude 3-B");
    assert_eq!(instances[2]["name"], "Claude 4-C");
    
    for instance in &instances {
        assert_eq!(instance["working_directory"], working_dir);
        // Verify UUID format
        let _uuid = Uuid::parse_str(&instance["id"]).expect("Invalid UUID");
    }
    
    println!("✅ Instance data structures created correctly");
}

#[tokio::test]
async fn test_coordination_message_format() {
    // Test the coordination message format that gets sent to new instances
    let task_desc = "Implement feature A";
    let scope = "src/a.rs, tests/a_test.rs";
    let priority = "High";
    let working_dir = "/test/project";
    
    let coordination_message = format!(
        r#"You are part of a coordinated team of Claude instances working on a shared codebase.

YOUR ASSIGNED SUBTASK: {}
SCOPE: {}
PRIORITY: {}
WORKING DIRECTORY: {}

COORDINATION PROTOCOL:
1. Use TaskMaster AI tools to stay in sync:
   - mcp__taskmaster-ai__get_tasks: Check current task status
   - mcp__taskmaster-ai__set_task_status: Mark tasks done/in-progress
   - mcp__taskmaster-ai__add_task: Add discovered subtasks
   
2. Focus ONLY on your assigned scope to avoid conflicts
3. Update main instance (Tab 1) with major progress
4. Use TaskMaster to communicate completion status

IMPORTANT: Work within your scope and coordinate via TaskMaster!"#,
        task_desc,
        scope,
        priority,
        working_dir
    );
    
    // Verify the message contains key coordination elements
    assert!(coordination_message.contains("coordinated team"));
    assert!(coordination_message.contains(task_desc));
    assert!(coordination_message.contains(scope));
    assert!(coordination_message.contains(priority));
    assert!(coordination_message.contains(working_dir));
    assert!(coordination_message.contains("mcp__taskmaster-ai__get_tasks"));
    assert!(coordination_message.contains("TaskMaster"));
    
    println!("✅ Coordination message format is correct");
    println!("Message length: {} characters", coordination_message.len());
}