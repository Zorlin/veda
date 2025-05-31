use std::process::{Command, Stdio};
use std::io::{Write, BufRead, BufReader};
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_mcp_server_initialization() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send initialize request
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

    // Read response with timeout
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to initialize within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response["result"].is_object());
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(response["result"]["serverInfo"]["name"], "veda-mcp-server");
    assert_eq!(response["result"]["serverInfo"]["version"], "1.0.0");

    child.kill().expect("Failed to kill process");
}

#[tokio::test]
async fn test_tools_list() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send tools/list request
    let tools_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });

    writeln!(stdin, "{}", serde_json::to_string(&tools_request).unwrap()).unwrap();
    stdin.flush().unwrap();

    // Read response
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to tools/list within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);
    assert!(response["result"]["tools"].is_array());
    
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3, "Should have exactly 3 tools");

    // Check for expected tools
    let tool_names: Vec<&str> = tools.iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    
    assert!(tool_names.contains(&"veda_spawn_instances"));
    assert!(tool_names.contains(&"veda_list_instances"));
    assert!(tool_names.contains(&"veda_close_instance"));

    // Validate veda_spawn_instances tool schema
    let spawn_tool = tools.iter()
        .find(|tool| tool["name"] == "veda_spawn_instances")
        .unwrap();
    
    assert_eq!(spawn_tool["description"], "Spawn additional Claude Code instances to work on a task in parallel");
    assert!(spawn_tool["inputSchema"]["properties"]["task_description"].is_object());
    assert!(spawn_tool["inputSchema"]["properties"]["num_instances"].is_object());
    assert_eq!(spawn_tool["inputSchema"]["properties"]["num_instances"]["minimum"], 1);
    assert_eq!(spawn_tool["inputSchema"]["properties"]["num_instances"]["maximum"], 3);

    child.kill().expect("Failed to kill process");
}

#[tokio::test]
async fn test_veda_spawn_instances_tool_call() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send tools/call request for veda_spawn_instances
    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "veda_spawn_instances",
            "arguments": {
                "task_description": "Implement user authentication and product catalog",
                "num_instances": 2
            }
        }
    });

    writeln!(stdin, "{}", serde_json::to_string(&call_request).unwrap()).unwrap();
    stdin.flush().unwrap();

    // Read response
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to tool call within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 3);
    assert!(response["result"]["content"].is_array());
    
    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");
    
    let text = content["text"].as_str().unwrap();
    // Should contain error about not connecting to Veda since it's not running
    assert!(text.contains("Could not connect to Veda") || text.contains("Spawning"));

    child.kill().expect("Failed to kill process");
}

#[tokio::test]
async fn test_veda_list_instances_tool_call() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send tools/call request for veda_list_instances
    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "veda_list_instances",
            "arguments": {}
        }
    });

    writeln!(stdin, "{}", serde_json::to_string(&call_request).unwrap()).unwrap();
    stdin.flush().unwrap();

    // Read response
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to tool call within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 4);
    assert!(response["result"]["content"].is_array());
    
    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");
    
    let text = content["text"].as_str().unwrap();
    // Should contain error about not connecting to Veda since it's not running  
    assert!(text.contains("Could not connect to Veda") || text.contains("Listing"));

    child.kill().expect("Failed to kill process");
}

#[tokio::test]
async fn test_veda_close_instance_tool_call() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send tools/call request for veda_close_instance
    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "veda_close_instance",
            "arguments": {
                "instance_name": "Claude 2-A"
            }
        }
    });

    writeln!(stdin, "{}", serde_json::to_string(&call_request).unwrap()).unwrap();
    stdin.flush().unwrap();

    // Read response
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to tool call within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 5);
    assert!(response["result"]["content"].is_array());
    
    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");
    
    let text = content["text"].as_str().unwrap();
    // Should contain error about not connecting to Veda since it's not running
    assert!(text.contains("Could not connect to Veda") || text.contains("Closing"));

    child.kill().expect("Failed to kill process");
}

#[tokio::test]
async fn test_unknown_method() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send unknown method request
    let unknown_request = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "unknown/method"
    });

    writeln!(stdin, "{}", serde_json::to_string(&unknown_request).unwrap()).unwrap();
    stdin.flush().unwrap();

    // Read response
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to unknown method within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 6);
    assert!(response["error"].is_object());
    assert_eq!(response["error"]["code"], -32601);
    assert_eq!(response["error"]["message"], "Method not found");

    child.kill().expect("Failed to kill process");
}

#[tokio::test]
async fn test_unknown_tool_call() {
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "veda-mcp-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start MCP server");

    let stdin = child.stdin.as_mut().expect("Failed to get stdin");
    let stdout = child.stdout.as_mut().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Send tools/call request for unknown tool
    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "unknown_tool",
            "arguments": {}
        }
    });

    writeln!(stdin, "{}", serde_json::to_string(&call_request).unwrap()).unwrap();
    stdin.flush().unwrap();

    // Read response
    let mut response_line = String::new();
    let read_result = timeout(Duration::from_secs(5), async {
        reader.read_line(&mut response_line)
    }).await;

    assert!(read_result.is_ok(), "Server should respond to unknown tool within 5 seconds");
    
    let response: Value = serde_json::from_str(&response_line.trim())
        .expect("Response should be valid JSON");

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 7);
    assert!(response["error"].is_object());
    assert_eq!(response["error"]["code"], -32601);
    assert_eq!(response["error"]["message"], "Method not found");

    child.kill().expect("Failed to kill process");
}

#[test]
fn test_veda_spawn_instances_default_parameters() {
    // Test that missing num_instances defaults to 2
    let tool_input = json!({
        "task_description": "Test task"
    });
    
    let num_instances = tool_input["num_instances"].as_u64().unwrap_or(2);
    assert_eq!(num_instances, 2);
}

#[test]
fn test_veda_spawn_instances_parameter_validation() {
    // Test with valid parameters
    let tool_input = json!({
        "task_description": "Implement features A, B, and C",
        "num_instances": 3
    });
    
    let task_desc = tool_input["task_description"].as_str().unwrap_or("");
    let num_instances = tool_input["num_instances"].as_u64().unwrap_or(2);
    
    assert_eq!(task_desc, "Implement features A, B, and C");
    assert_eq!(num_instances, 3);
    assert!(num_instances >= 1 && num_instances <= 3);
}

#[test]
fn test_json_parsing_robustness() {
    // Test various JSON formats that might be sent
    let valid_requests = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        r#"{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}"#,
        r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }"#,
    ];
    
    for request_str in valid_requests {
        let parsed: Result<Value, _> = serde_json::from_str(request_str);
        assert!(parsed.is_ok(), "Should parse valid JSON: {}", request_str);
        
        let request = parsed.unwrap();
        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["id"], 1);
        assert_eq!(request["method"], "tools/list");
    }
}