use std::io::{self, BufRead, Write};
use serde_json::{json, Value};

// Extracted functions for testability
pub fn create_tools_list_response(request_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "tools": [
                {
                    "name": "veda_spawn_instances",
                    "description": "Spawn additional Claude Code instances to work on a task in parallel",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "task_description": {
                                "type": "string",
                                "description": "Description of the task that will be divided among instances"
                            },
                            "num_instances": {
                                "type": "number",
                                "description": "Number of additional instances to spawn (1-3)",
                                "minimum": 1,
                                "maximum": 3
                            }
                        },
                        "required": ["task_description"]
                    }
                },
                {
                    "name": "veda_list_instances",
                    "description": "List all currently active Claude Code instances",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "veda_close_instance",
                    "description": "Close a specific Claude Code instance by name",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "instance_name": {
                                "type": "string",
                                "description": "Name of the instance to close (e.g., 'Claude 2-A')"
                            }
                        },
                        "required": ["instance_name"]
                    }
                }
            ]
        }
    })
}

pub fn create_tool_call_response(request_id: &Value, tool_name: &str, tool_input: &Value) -> Value {
    match tool_name {
        "veda_spawn_instances" => {
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": format!(
                                "✅ Request sent to Veda to spawn {} instances for task: {}",
                                tool_input["num_instances"].as_u64().unwrap_or(2),
                                tool_input["task_description"].as_str().unwrap_or("")
                            )
                        }
                    ]
                }
            })
        }
        "veda_list_instances" => {
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": "✅ Request sent to Veda to list all instances"
                        }
                    ]
                }
            })
        }
        "veda_close_instance" => {
            json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": format!(
                                "✅ Request sent to Veda to close instance: {}",
                                tool_input["instance_name"].as_str().unwrap_or("")
                            )
                        }
                    ]
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

pub fn create_initialize_response(request_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "veda-mcp-server",
                "version": "1.0.0"
            }
        }
    })
}

pub fn create_error_response(request_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    })
}

pub fn process_request(request: &Value) -> Value {
    match request["method"].as_str() {
        Some("tools/list") => create_tools_list_response(&request["id"]),
        Some("tools/call") => {
            let tool_name = request["params"]["name"].as_str().unwrap_or("");
            let tool_input = &request["params"]["arguments"];
            create_tool_call_response(&request["id"], tool_name, tool_input)
        }
        Some("initialize") => create_initialize_response(&request["id"]),
        _ => create_error_response(&request["id"]),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    
    for line in stdin.lock().lines() {
        let line = line?;
        let request: Value = serde_json::from_str(&line)?;
        let response = process_request(&request);
        
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_create_tools_list_response() {
        let request_id = json!(42);
        let response = create_tools_list_response(&request_id);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 42);
        assert!(response["result"]["tools"].is_array());
        
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        
        let tool_names: Vec<&str> = tools.iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        
        assert!(tool_names.contains(&"veda_spawn_instances"));
        assert!(tool_names.contains(&"veda_list_instances"));
        assert!(tool_names.contains(&"veda_close_instance"));
    }

    #[test]
    fn test_create_tool_call_response_spawn_instances() {
        let request_id = json!(1);
        let tool_input = json!({
            "task_description": "Test task",
            "num_instances": 3
        });
        
        let response = create_tool_call_response(&request_id, "veda_spawn_instances", &tool_input);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("spawn 3 instances"));
        assert!(text.contains("Test task"));
    }

    #[test]
    fn test_create_tool_call_response_spawn_instances_default() {
        let request_id = json!(1);
        let tool_input = json!({
            "task_description": "Test task"
            // num_instances omitted, should default to 2
        });
        
        let response = create_tool_call_response(&request_id, "veda_spawn_instances", &tool_input);
        
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("spawn 2 instances"));
    }

    #[test]
    fn test_create_tool_call_response_list_instances() {
        let request_id = json!(2);
        let tool_input = json!({});
        
        let response = create_tool_call_response(&request_id, "veda_list_instances", &tool_input);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 2);
        
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("list all instances"));
    }

    #[test]
    fn test_create_tool_call_response_close_instance() {
        let request_id = json!(3);
        let tool_input = json!({
            "instance_name": "Claude 2-A"
        });
        
        let response = create_tool_call_response(&request_id, "veda_close_instance", &tool_input);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 3);
        
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("close instance: Claude 2-A"));
    }

    #[test]
    fn test_create_tool_call_response_unknown_tool() {
        let request_id = json!(4);
        let tool_input = json!({});
        
        let response = create_tool_call_response(&request_id, "unknown_tool", &tool_input);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 4);
        assert!(response["error"].is_object());
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn test_create_initialize_response() {
        let request_id = json!("init-1");
        let response = create_initialize_response(&request_id);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "init-1");
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "veda-mcp-server");
        assert_eq!(response["result"]["serverInfo"]["version"], "1.0.0");
    }

    #[test]
    fn test_create_error_response() {
        let request_id = json!(999);
        let response = create_error_response(&request_id);
        
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 999);
        assert!(response["error"].is_object());
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["error"]["message"], "Method not found");
    }

    #[test]
    fn test_process_request_tools_list() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        });
        
        let response = process_request(&request);
        assert_eq!(response["id"], 1);
        assert!(response["result"]["tools"].is_array());
    }

    #[test]
    fn test_process_request_tools_call() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "veda_spawn_instances",
                "arguments": {
                    "task_description": "Test",
                    "num_instances": 1
                }
            }
        });
        
        let response = process_request(&request);
        assert_eq!(response["id"], 2);
        assert!(response["result"]["content"].is_array());
    }

    #[test]
    fn test_process_request_initialize() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "initialize",
            "params": {}
        });
        
        let response = process_request(&request);
        assert_eq!(response["id"], 3);
        assert_eq!(response["result"]["serverInfo"]["name"], "veda-mcp-server");
    }

    #[test]
    fn test_process_request_unknown_method() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "unknown/method"
        });
        
        let response = process_request(&request);
        assert_eq!(response["id"], 4);
        assert!(response["error"].is_object());
    }

    #[test]
    fn test_parameter_validation() {
        // Test empty task description
        let tool_input = json!({
            "task_description": "",
            "num_instances": 2
        });
        
        let task_desc = tool_input["task_description"].as_str().unwrap_or("");
        let num_instances = tool_input["num_instances"].as_u64().unwrap_or(2);
        
        assert_eq!(task_desc, "");
        assert_eq!(num_instances, 2);
        
        // Test missing instance name
        let tool_input = json!({});
        let instance_name = tool_input["instance_name"].as_str().unwrap_or("");
        assert_eq!(instance_name, "");
    }

    #[test]
    fn test_boundary_values() {
        // Test minimum num_instances
        let tool_input = json!({
            "task_description": "Test",
            "num_instances": 1
        });
        
        let num_instances = tool_input["num_instances"].as_u64().unwrap_or(2);
        assert_eq!(num_instances, 1);
        
        // Test maximum num_instances
        let tool_input = json!({
            "task_description": "Test",
            "num_instances": 3
        });
        
        let num_instances = tool_input["num_instances"].as_u64().unwrap_or(2);
        assert_eq!(num_instances, 3);
        
        // Test beyond maximum (should still parse but validation would happen elsewhere)
        let tool_input = json!({
            "task_description": "Test",
            "num_instances": 10
        });
        
        let num_instances = tool_input["num_instances"].as_u64().unwrap_or(2);
        assert_eq!(num_instances, 10);
    }
}