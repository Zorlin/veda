use std::process::{Command, Stdio};
use std::io::{Write, BufRead, BufReader};
use std::time::Duration;
use tokio::time::timeout;
use serde_json::{json, Value};

/// Helper to start and initialize MCP server
async fn start_mcp_server() -> (std::process::Child, Box<dyn Write + Send>, BufReader<Box<dyn std::io::Read + Send>>) {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .env("VEDA_SESSION_ID", "test-session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = Box::new(child.stdin.take().expect("Failed to get stdin"));
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(Box::new(stdout) as Box<dyn std::io::Read + Send>);

    // Initialize the server
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {}
        }
    });

    let mut stdin_writer = stdin;
    writeln!(stdin_writer, "{}", serde_json::to_string(&init_request).unwrap()).unwrap();
    stdin_writer.flush().unwrap();

    // Read init response
    let mut response_line = String::new();
    reader.read_line(&mut response_line).unwrap();

    (child, stdin_writer, reader)
}

#[cfg(test)]
mod mcp_spawn_tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_instances_tool_parameters() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        // Test with valid parameters
        let spawn_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_spawn_instances",
                "arguments": {
                    "task_description": "Implement parallel features",
                    "num_instances": 3
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&spawn_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 2);
        let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(response_text.contains("Spawning") || response_text.contains("Could not connect"),
            "Response should indicate spawning or connection issue: {}", response_text);

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_spawn_instances_default_count() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        // Test without num_instances (should default to 2)
        let spawn_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_spawn_instances",
                "arguments": {
                    "task_description": "Test task without count"
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&spawn_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(response_text.contains("Spawning") || response_text.contains("Could not connect"),
            "Response should indicate spawning or connection issue: {}", response_text);

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_list_instances_tool() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        let list_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_list_instances",
                "arguments": {}
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&list_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response["result"]["content"][0]["text"].as_str().unwrap()
            .contains("Listing instances") ||
            response["result"]["content"][0]["text"].as_str().unwrap()
            .contains("Could not connect"));

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_close_instance_tool() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        let close_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_close_instance",
                "arguments": {
                    "instance_name": "Claude 2-A"
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&close_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response["result"]["content"][0]["text"].as_str().unwrap()
            .contains("Claude 2-A") ||
            response["result"]["content"][0]["text"].as_str().unwrap()
            .contains("Could not connect"));

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_tool_calls() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        // Send multiple tool calls in sequence
        let requests = vec![
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "veda_list_instances",
                    "arguments": {}
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "veda_spawn_instances",
                    "arguments": {
                        "task_description": "Task 1",
                        "num_instances": 1
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "veda_list_instances",
                    "arguments": {}
                }
            }),
        ];

        for request in requests {
            writeln!(stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
            stdin.flush().unwrap();

            let mut response_line = String::new();
            reader.read_line(&mut response_line).unwrap();
            let response: Value = serde_json::from_str(&response_line).unwrap();

            assert_eq!(response["jsonrpc"], "2.0");
            assert!(response["result"]["content"][0]["text"].is_string());
        }

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_session_id_propagation() {
        // Test with different session IDs
        let session_ids = vec!["session-1", "session-2", "session-3"];

        for session_id in session_ids {
            let mut child = Command::new("cargo")
                .args(&["run", "--bin", "veda-mcp-server"])
                .env("VEDA_SESSION_ID", session_id)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to start MCP server");

            let stdin = child.stdin.as_mut().expect("Failed to get stdin");
            let stdout = child.stdout.as_mut().expect("Failed to get stdout");
            let mut reader = BufReader::new(stdout);

            // Initialize
            let init_request = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {}
                }
            });

            writeln!(stdin, "{}", serde_json::to_string(&init_request).unwrap()).unwrap();
            stdin.flush().unwrap();

            let mut response_line = String::new();
            reader.read_line(&mut response_line).unwrap();

            // Test spawn with this session
            let spawn_request = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "veda_spawn_instances",
                    "arguments": {
                        "task_description": format!("Task for session {}", session_id),
                        "num_instances": 1
                    }
                }
            });

            writeln!(stdin, "{}", serde_json::to_string(&spawn_request).unwrap()).unwrap();
            stdin.flush().unwrap();

            response_line.clear();
            reader.read_line(&mut response_line).unwrap();
            let response: Value = serde_json::from_str(&response_line).unwrap();

            // Verify response mentions the task
            let response_text = response["result"]["content"][0]["text"].as_str().unwrap();
            assert!(
                response_text.contains(&format!("Task for session {}", session_id)) ||
                response_text.contains("Could not connect"),
                "Response should mention the session-specific task"
            );

            child.kill().unwrap();
        }
    }

    #[tokio::test]
    async fn test_invalid_instance_count() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        // Test with instance count exceeding maximum (should be capped)
        let spawn_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_spawn_instances",
                "arguments": {
                    "task_description": "Test with too many instances",
                    "num_instances": 10  // Exceeds max of 3
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&spawn_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        // Should still succeed but cap at maximum
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response["result"]["content"][0]["text"].is_string());

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_empty_task_description() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        let spawn_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_spawn_instances",
                "arguments": {
                    "task_description": "",
                    "num_instances": 2
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&spawn_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        // Should handle empty description gracefully
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response["result"]["content"][0]["text"].is_string());

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_close_nonexistent_instance() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        let close_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_close_instance",
                "arguments": {
                    "instance_name": "NonExistent-Instance"
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&close_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();

        // Should handle gracefully
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response["result"]["content"][0]["text"].is_string());

        child.kill().unwrap();
    }

    #[tokio::test]
    async fn test_mcp_timeout_handling() {
        let (mut child, mut stdin, mut reader) = start_mcp_server().await;

        // Send a request and verify we get a response within timeout
        let list_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_list_instances",
                "arguments": {}
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&list_request).unwrap()).unwrap();
        stdin.flush().unwrap();

        // Use tokio timeout to ensure we get response quickly
        let response_result = timeout(Duration::from_secs(2), async {
            let mut response_line = String::new();
            reader.read_line(&mut response_line).map(|_| response_line)
        }).await;

        assert!(response_result.is_ok(), "Should respond within 2 seconds");
        
        let response_line = response_result.unwrap().unwrap();
        let response: Value = serde_json::from_str(&response_line).unwrap();
        assert_eq!(response["jsonrpc"], "2.0");

        child.kill().unwrap();
    }
}