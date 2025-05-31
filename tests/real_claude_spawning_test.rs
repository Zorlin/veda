use std::time::Duration;
use tokio::time::timeout;
use tokio::process::Command;
use uuid::Uuid;

#[tokio::test]
async fn test_real_claude_process_spawn() {
    // Test that we can actually spawn a Claude process with the correct arguments
    
    let test_message = "Hello, this is a test message. Please respond briefly.";
    let instance_id = Uuid::new_v4();
    
    // Build the same command that send_to_claude_with_session uses
    let mut cmd = Command::new("claude");
    
    // Set VEDA_SESSION_ID if available
    if let Ok(veda_session_id) = std::env::var("VEDA_SESSION_ID") {
        cmd.env("VEDA_SESSION_ID", veda_session_id);
    }
    
    cmd.arg("-p")
        .arg(&test_message)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--mcp-config")
        .arg(".mcp.json");
    
    println!("Attempting to spawn Claude with command: {:?}", cmd.as_std());
    
    // Try to spawn the process (but don't actually run it to completion to avoid costs)
    match cmd.spawn() {
        Ok(mut child) => {
            println!("✅ Claude process spawned successfully!");
            
            // Kill the process immediately to avoid running a full conversation
            if let Err(e) = child.kill().await {
                println!("Warning: Failed to kill test process: {}", e);
            }
            
            // Wait for it to exit
            let _ = child.wait().await;
            println!("✅ Test process cleaned up");
        }
        Err(e) => {
            panic!("❌ Failed to spawn Claude process: {}. Make sure Claude is installed and available in PATH.", e);
        }
    }
}

#[tokio::test]
async fn test_claude_command_validation() {
    // Test that the Claude command exists and is accessible
    
    let output = timeout(Duration::from_secs(5), Command::new("claude").arg("--version").output()).await;
    
    match output {
        Ok(Ok(output)) => {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                println!("✅ Claude is available, version: {}", version.trim());
            } else {
                let error = String::from_utf8_lossy(&output.stderr);
                println!("❌ Claude command failed: {}", error);
                panic!("Claude --version failed");
            }
        }
        Ok(Err(e)) => {
            panic!("❌ Failed to run claude --version: {}. Make sure Claude is installed.", e);
        }
        Err(_) => {
            panic!("❌ claude --version timed out");
        }
    }
}

#[tokio::test]
async fn test_mcp_config_exists() {
    // Test that the MCP config file exists
    use std::path::Path;
    
    let mcp_config_path = Path::new(".mcp.json");
    
    if mcp_config_path.exists() {
        println!("✅ .mcp.json file exists");
        
        // Try to read and parse it
        match std::fs::read_to_string(mcp_config_path) {
            Ok(content) => {
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(config) => {
                        println!("✅ .mcp.json is valid JSON");
                        if let Some(mcpServers) = config.get("mcpServers") {
                            println!("✅ MCP servers configured: {}", mcpServers);
                        }
                    }
                    Err(e) => {
                        println!("❌ .mcp.json is not valid JSON: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Could not read .mcp.json: {}", e);
            }
        }
    } else {
        println!("⚠️  .mcp.json file does not exist - this might affect MCP functionality");
    }
}

#[tokio::test] 
async fn test_working_directory_context() {
    // Test that we're in the right working directory for tests
    use std::env;
    
    let current_dir = env::current_dir().expect("Could not get current directory");
    println!("Current working directory: {:?}", current_dir);
    
    // Check that we're in the veda project directory
    let cargo_toml = current_dir.join("Cargo.toml");
    assert!(cargo_toml.exists(), "Cargo.toml should exist in current directory");
    
    // Read Cargo.toml to verify it's the veda project
    let cargo_content = std::fs::read_to_string(&cargo_toml).expect("Could not read Cargo.toml");
    assert!(cargo_content.contains("veda-tui"), "Should be in the veda-tui project directory");
    
    println!("✅ Running in correct project directory");
}

#[cfg(feature = "dangerous_live_test")]
#[tokio::test]
async fn test_full_claude_interaction() {
    // This test actually runs Claude and checks for a response
    // Only run with: cargo test --features dangerous_live_test
    // WARNING: This will make an actual API call to Claude
    
    use tokio::process::{Command, Stdio};
    use tokio::io::{AsyncBufReadExt, BufReader};
    
    let test_message = "Please respond with exactly the text 'TEST_RESPONSE_OK' and nothing else.";
    
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(&test_message)
        .arg("--output-format")
        .arg("stream-json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    
    let mut child = cmd.spawn().expect("Failed to spawn Claude");
    
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout).lines();
    
    let mut found_response = false;
    
    // Read lines with timeout
    while let Ok(Some(line)) = timeout(Duration::from_secs(30), reader.next_line()).await {
        if let Ok(line) = line {
            println!("Claude output: {}", line);
            
            // Parse JSON response
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(text) = json.get("type").and_then(|t| {
                    if t == "assistant" {
                        json.get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|first| first.get("text"))
                            .and_then(|t| t.as_str())
                    } else {
                        None
                    }
                }) {
                    if text.contains("TEST_RESPONSE_OK") {
                        found_response = true;
                        break;
                    }
                }
            }
        }
    }
    
    // Kill the process
    let _ = child.kill().await;
    let _ = child.wait().await;
    
    assert!(found_response, "Did not receive expected response from Claude");
    println!("✅ Full Claude interaction test passed");
}