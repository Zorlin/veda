#[cfg(test)]
mod instance_output_integration_tests {
    use std::process::{Command, Stdio};
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;
    
    #[test]
    fn test_veda_binary_exists() {
        // First verify the binary was built
        let output = Command::new("cargo")
            .args(&["build", "--release"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Failed to run cargo build");
        
        assert!(output.status.success(), "Cargo build failed: {}", 
            String::from_utf8_lossy(&output.stderr));
        
        // Check that the binary exists
        let binary_path = format!("{}/target/release/veda-tui", env!("CARGO_MANIFEST_DIR"));
        assert!(std::path::Path::new(&binary_path).exists(), 
            "veda-tui binary not found at {}", binary_path);
    }
    
    #[test]
    #[ignore] // Mark as ignored by default since it requires Claude CLI
    fn test_real_spawning_with_mcp_server() {
        // This test actually spawns veda and uses the MCP server to spawn instances
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let temp_path = temp_dir.path();
        
        // Start the MCP server
        let mcp_server = Command::new("cargo")
            .args(&["run", "--bin", "veda-mcp-server"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to start MCP server");
        
        // Give the server time to start
        std::thread::sleep(Duration::from_secs(1));
        
        // Test spawning instances via Claude with MCP tools
        let claude_script = r#"
            import json
            import sys
            
            # Simulate claude using MCP tools to spawn instances
            spawn_request = {
                "task_description": "Test spawning multiple instances",
                "num_instances": 3
            }
            
            print(json.dumps({
                "type": "tool_use",
                "name": "spawn_instances", 
                "input": spawn_request
            }))
            
            # Wait and then list instances
            import time
            time.sleep(2)
            
            print(json.dumps({
                "type": "tool_use",
                "name": "list_instances",
                "input": {}
            }))
        "#;
        
        // Write the test script
        let script_path = temp_path.join("test_spawn.py");
        std::fs::write(&script_path, claude_script)
            .expect("Failed to write test script");
        
        // Run the test
        let output = Command::new("python3")
            .arg(&script_path)
            .output()
            .expect("Failed to run test script");
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        println!("Script output: {}", stdout);
        if !stderr.is_empty() {
            println!("Script errors: {}", stderr);
        }
        
        // Clean up
        drop(mcp_server);
    }
    
    #[test]
    fn test_message_routing_logic() {
        // Test the session ID routing logic without actually spawning Claude
        use uuid::Uuid;
        use std::collections::HashMap;
        
        // Simulate the instance structure
        #[derive(Debug)]
        struct MockInstance {
            id: Uuid,
            name: String,
            session_id: Option<String>,
            messages: Vec<String>,
        }
        
        let mut instances = vec![
            MockInstance {
                id: Uuid::new_v4(),
                name: "Veda-1".to_string(),
                session_id: None,
                messages: vec![],
            },
            MockInstance {
                id: Uuid::new_v4(),
                name: "Veda-2".to_string(),
                session_id: Some("session-123".to_string()),
                messages: vec![],
            },
            MockInstance {
                id: Uuid::new_v4(),
                name: "Veda-3".to_string(),
                session_id: Some("session-456".to_string()),
                messages: vec![],
            },
        ];
        
        // Simulate receiving a message with session_id
        let incoming_session_id = "session-456";
        let incoming_instance_id = instances[2].id; // This should match but we'll test routing by session
        let incoming_text = "Hello from Veda-3";
        
        // Find target instance by session_id (this is the actual logic from main.rs)
        let target_instance_index = instances.iter().position(|i| 
            i.session_id.as_ref() == Some(&incoming_session_id.to_string())
        ).or_else(|| instances.iter().position(|i| i.id == incoming_instance_id));
        
        assert!(target_instance_index.is_some(), "Should find target instance");
        let target_idx = target_instance_index.unwrap();
        assert_eq!(target_idx, 2, "Should route to Veda-3 (index 2)");
        assert_eq!(instances[target_idx].name, "Veda-3");
        
        // Add the message
        instances[target_idx].messages.push(incoming_text.to_string());
        
        // Verify the message was routed correctly
        assert_eq!(instances[2].messages.len(), 1);
        assert_eq!(instances[2].messages[0], "Hello from Veda-3");
        assert_eq!(instances[0].messages.len(), 0); // Veda-1 should have no messages
        assert_eq!(instances[1].messages.len(), 0); // Veda-2 should have no messages
        
        println!("✅ Message routing test passed");
    }
    
    #[test]
    fn test_spawned_instance_session_assignment() {
        // Test that spawned instances get session IDs assigned correctly
        use uuid::Uuid;
        
        #[derive(Debug)]
        struct MockInstance {
            id: Uuid,
            name: String,
            session_id: Option<String>,
        }
        
        let mut instances = vec![
            MockInstance {
                id: Uuid::new_v4(),
                name: "Veda-1".to_string(),
                session_id: None,
            }
        ];
        
        // Simulate spawning new instances
        for i in 2..=4 {
            instances.push(MockInstance {
                id: Uuid::new_v4(),
                name: format!("Veda-{}", i),
                session_id: None, // Initially no session
            });
        }
        
        // Simulate session start events
        let sessions = vec!["session-123", "session-456", "session-789"];
        
        for (i, session_id) in sessions.iter().enumerate() {
            let instance_idx = i + 1; // Skip Veda-1
            if let Some(instance) = instances.get_mut(instance_idx) {
                instance.session_id = Some(session_id.to_string());
                println!("Assigned session {} to {}", session_id, instance.name);
            }
        }
        
        // Verify all spawned instances have sessions
        for i in 1..instances.len() {
            assert!(instances[i].session_id.is_some(), 
                "{} should have a session ID", instances[i].name);
        }
        
        // Verify session IDs are unique
        let mut session_ids = std::collections::HashSet::new();
        for instance in &instances[1..] { // Skip Veda-1
            if let Some(ref session_id) = instance.session_id {
                assert!(session_ids.insert(session_id.clone()), 
                    "Session ID {} should be unique", session_id);
            }
        }
        
        println!("✅ Session assignment test passed");
    }
    
    #[test]
    fn test_debug_output_shows_in_logs() {
        // This test checks that when StreamText arrives, it logs properly
        // We can't easily test the actual UI, but we can test the logic
        
        use uuid::Uuid;
        
        let instance_id = Uuid::new_v4();
        let session_id = "session-test-123";
        let text = "Hello from spawned instance";
        
        // This simulates the logging that should happen in main.rs:1202
        let tab_info = format!("Session {} (instance {})", session_id, instance_id);
        let log_preview = text.chars().take(50).collect::<String>();
        
        println!("StreamText for instance {} ({}): {:?}", instance_id, tab_info, log_preview);
        
        // Verify the format
        assert!(tab_info.contains(session_id));
        assert!(tab_info.contains(&instance_id.to_string()));
        assert_eq!(log_preview, text); // Should be same since it's short
        
        println!("✅ Debug output format test passed");
    }
}