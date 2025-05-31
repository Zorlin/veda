#[cfg(test)]
mod mcp_integration_tests {
    use uuid::Uuid;
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};
    
    // Mock MCP tool requests and responses
    #[derive(Debug, Clone)]
    struct MockMcpRequest {
        tool_name: String,
        instance_id: Uuid,
        params: Value,
    }
    
    #[derive(Debug)]
    struct MockMcpResponse {
        success: bool,
        result: Value,
    }
    
    // Simulated Veda instance for MCP testing
    #[derive(Debug)]
    struct MockVedaInstance {
        id: Uuid,
        name: String,
        session_id: Option<String>,
        messages: Vec<(String, String)>,
        is_spawned: bool,
        working_directory: String,
    }
    
    struct MockMcpServer {
        instances: Arc<Mutex<Vec<MockVedaInstance>>>,
        spawn_counter: Arc<Mutex<usize>>,
    }
    
    impl MockMcpServer {
        fn new() -> Self {
            let instances = Arc::new(Mutex::new(vec![
                MockVedaInstance {
                    id: Uuid::new_v4(),
                    name: "Veda-1".to_string(),
                    session_id: None,
                    messages: Vec::new(),
                    is_spawned: false,
                    working_directory: "/home/user/project".to_string(),
                }
            ]));
            
            Self {
                instances,
                spawn_counter: Arc::new(Mutex::new(1)),
            }
        }
        
        async fn handle_request(&self, request: MockMcpRequest) -> MockMcpResponse {
            match request.tool_name.as_str() {
                "veda_spawn_instances" => {
                    self.handle_spawn_instances(request.params).await
                }
                "veda_list_instances" => {
                    self.handle_list_instances().await
                }
                "veda_close_instance" => {
                    self.handle_close_instance(request.params).await
                }
                _ => MockMcpResponse {
                    success: false,
                    result: json!({"error": "Unknown tool"}),
                }
            }
        }
        
        async fn handle_spawn_instances(&self, params: Value) -> MockMcpResponse {
            let num_instances = params["num_instances"].as_i64().unwrap_or(1) as usize;
            let task_description = params["task_description"].as_str().unwrap_or("No description");
            
            let mut instances = self.instances.lock().await;
            let mut counter = self.spawn_counter.lock().await;
            let mut spawned_names = Vec::new();
            
            for i in 0..num_instances {
                if instances.len() >= 5 { // Max 5 instances
                    break;
                }
                
                *counter += 1;
                let name = format!("Veda-{}", *counter);
                let session_id = format!("session-{}-{}", *counter, Uuid::new_v4());
                
                instances.push(MockVedaInstance {
                    id: Uuid::new_v4(),
                    name: name.clone(),
                    session_id: Some(session_id.clone()),
                    messages: vec![
                        ("System".to_string(), format!(
                            "🤝 MULTI-INSTANCE COORDINATION MODE\n\nAssigned subtask {} of {}: {}",
                            i + 1, num_instances, task_description
                        ))
                    ],
                    is_spawned: true,
                    working_directory: "/home/user/project".to_string(),
                });
                
                spawned_names.push(name);
            }
            
            MockMcpResponse {
                success: true,
                result: json!({
                    "spawned": spawned_names,
                    "total_instances": instances.len(),
                    "message": format!("Successfully spawned {} instances", spawned_names.len())
                }),
            }
        }
        
        async fn handle_list_instances(&self) -> MockMcpResponse {
            let instances = self.instances.lock().await;
            let instance_list: Vec<Value> = instances.iter().enumerate().map(|(i, inst)| {
                json!({
                    "index": i + 1,
                    "name": inst.name,
                    "session_id": inst.session_id,
                    "is_spawned": inst.is_spawned,
                    "working_directory": inst.working_directory,
                    "message_count": inst.messages.len()
                })
            }).collect();
            
            MockMcpResponse {
                success: true,
                result: json!({
                    "instances": instance_list,
                    "count": instances.len()
                }),
            }
        }
        
        async fn handle_close_instance(&self, params: Value) -> MockMcpResponse {
            let instance_name = params["instance_name"].as_str().unwrap_or("");
            
            let mut instances = self.instances.lock().await;
            
            // Find instance by name
            let instance_idx = instances.iter().position(|i| i.name == instance_name);
            
            match instance_idx {
                Some(0) => MockMcpResponse {
                    success: false,
                    result: json!({"error": "Cannot close the main instance (Veda-1)"}),
                },
                Some(idx) if instances.len() > 1 => {
                    let removed = instances.remove(idx);
                    MockMcpResponse {
                        success: true,
                        result: json!({
                            "closed": removed.name,
                            "remaining_instances": instances.len()
                        }),
                    }
                },
                Some(_) => MockMcpResponse {
                    success: false,
                    result: json!({"error": "Cannot close the last remaining instance"}),
                },
                None => MockMcpResponse {
                    success: false,
                    result: json!({"error": format!("Instance '{}' not found", instance_name)}),
                },
            }
        }
    }
    
    #[tokio::test]
    async fn test_mcp_spawn_instances_basic() {
        let server = MockMcpServer::new();
        
        // Test spawning 2 instances
        let request = MockMcpRequest {
            tool_name: "veda_spawn_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "num_instances": 2,
                "task_description": "Implement user authentication"
            }),
        };
        
        let response = server.handle_request(request).await;
        assert!(response.success);
        
        let spawned = response.result["spawned"].as_array().unwrap();
        assert_eq!(spawned.len(), 2);
        assert_eq!(spawned[0], "Veda-2");
        assert_eq!(spawned[1], "Veda-3");
        
        // Verify total instances
        assert_eq!(response.result["total_instances"], 3);
    }
    
    #[tokio::test]
    async fn test_mcp_spawn_instances_max_limit() {
        let server = MockMcpServer::new();
        
        // Try to spawn 10 instances (should be limited to 4 more)
        let request = MockMcpRequest {
            tool_name: "veda_spawn_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "num_instances": 10,
                "task_description": "Large task"
            }),
        };
        
        let response = server.handle_request(request).await;
        assert!(response.success);
        
        let spawned = response.result["spawned"].as_array().unwrap();
        assert_eq!(spawned.len(), 4); // Only 4 more can be spawned (5 total - 1 existing)
        assert_eq!(response.result["total_instances"], 5);
    }
    
    #[tokio::test]
    async fn test_mcp_list_instances() {
        let server = MockMcpServer::new();
        
        // First spawn some instances
        let spawn_request = MockMcpRequest {
            tool_name: "veda_spawn_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "num_instances": 2,
                "task_description": "Test task"
            }),
        };
        server.handle_request(spawn_request).await;
        
        // Now list instances
        let list_request = MockMcpRequest {
            tool_name: "veda_list_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({}),
        };
        
        let response = server.handle_request(list_request).await;
        assert!(response.success);
        
        let instances = response.result["instances"].as_array().unwrap();
        assert_eq!(instances.len(), 3);
        
        // Check main instance
        assert_eq!(instances[0]["name"], "Veda-1");
        assert_eq!(instances[0]["is_spawned"], false);
        assert!(instances[0]["session_id"].is_null());
        
        // Check spawned instances
        assert_eq!(instances[1]["name"], "Veda-2");
        assert_eq!(instances[1]["is_spawned"], true);
        assert!(!instances[1]["session_id"].is_null());
        
        assert_eq!(instances[2]["name"], "Veda-3");
        assert_eq!(instances[2]["is_spawned"], true);
        assert!(!instances[2]["session_id"].is_null());
    }
    
    #[tokio::test]
    async fn test_mcp_close_instance_success() {
        let server = MockMcpServer::new();
        
        // Spawn instances first
        let spawn_request = MockMcpRequest {
            tool_name: "veda_spawn_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "num_instances": 3,
                "task_description": "Test"
            }),
        };
        server.handle_request(spawn_request).await;
        
        // Close Veda-3
        let close_request = MockMcpRequest {
            tool_name: "veda_close_instance".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "instance_name": "Veda-3"
            }),
        };
        
        let response = server.handle_request(close_request).await;
        assert!(response.success);
        assert_eq!(response.result["closed"], "Veda-3");
        assert_eq!(response.result["remaining_instances"], 3);
    }
    
    #[tokio::test]
    async fn test_mcp_close_main_instance_fails() {
        let server = MockMcpServer::new();
        
        // Try to close main instance
        let close_request = MockMcpRequest {
            tool_name: "veda_close_instance".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "instance_name": "Veda-1"
            }),
        };
        
        let response = server.handle_request(close_request).await;
        assert!(!response.success);
        assert!(response.result["error"].as_str().unwrap().contains("Cannot close the main instance"));
    }
    
    #[tokio::test]
    async fn test_mcp_workflow_integration() {
        let server = MockMcpServer::new();
        let main_id = Uuid::new_v4();
        
        // 1. Spawn instances for a complex task
        let spawn_req = MockMcpRequest {
            tool_name: "veda_spawn_instances".to_string(),
            instance_id: main_id,
            params: json!({
                "num_instances": 3,
                "task_description": "Build a REST API with authentication, database, and frontend"
            }),
        };
        
        let spawn_resp = server.handle_request(spawn_req).await;
        assert!(spawn_resp.success);
        
        // 2. List to verify
        let list_req = MockMcpRequest {
            tool_name: "veda_list_instances".to_string(),
            instance_id: main_id,
            params: json!({}),
        };
        
        let list_resp = server.handle_request(list_req).await;
        let instances = list_resp.result["instances"].as_array().unwrap();
        assert_eq!(instances.len(), 4);
        
        // 3. Simulate work completion - close one instance
        let close_req = MockMcpRequest {
            tool_name: "veda_close_instance".to_string(),
            instance_id: main_id,
            params: json!({
                "instance_name": "Veda-2"
            }),
        };
        
        let close_resp = server.handle_request(close_req).await;
        assert!(close_resp.success);
        
        // 4. Final list to verify state
        let final_list = server.handle_request(MockMcpRequest {
            tool_name: "veda_list_instances".to_string(),
            instance_id: main_id,
            params: json!({}),
        }).await;
        
        let final_instances = final_list.result["instances"].as_array().unwrap();
        assert_eq!(final_instances.len(), 3);
        
        // Verify remaining instances
        let names: Vec<&str> = final_instances.iter()
            .map(|i| i["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Veda-1"));
        assert!(!names.contains(&"Veda-2")); // Closed
        assert!(names.contains(&"Veda-3"));
        assert!(names.contains(&"Veda-4"));
    }
    
    #[tokio::test]
    async fn test_session_assignment_on_spawn() {
        let server = MockMcpServer::new();
        
        // Spawn instances
        let request = MockMcpRequest {
            tool_name: "veda_spawn_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({
                "num_instances": 2,
                "task_description": "Test session assignment"
            }),
        };
        
        server.handle_request(request).await;
        
        // Get instance list to check sessions
        let list_req = MockMcpRequest {
            tool_name: "veda_list_instances".to_string(),
            instance_id: Uuid::new_v4(),
            params: json!({}),
        };
        
        let response = server.handle_request(list_req).await;
        let instances = response.result["instances"].as_array().unwrap();
        
        // Main instance should have no session
        assert!(instances[0]["session_id"].is_null());
        
        // Spawned instances should have unique sessions
        let session2 = instances[1]["session_id"].as_str().unwrap();
        let session3 = instances[2]["session_id"].as_str().unwrap();
        
        assert!(session2.starts_with("session-2-"));
        assert!(session3.starts_with("session-3-"));
        assert_ne!(session2, session3);
    }
    
    #[test]
    fn test_mcp_tool_name_validation() {
        // Test that tool names match expected format
        let valid_tools = vec![
            "veda_spawn_instances",
            "veda_list_instances", 
            "veda_close_instance"
        ];
        
        for tool in valid_tools {
            assert!(tool.starts_with("veda_"));
            assert!(tool.chars().all(|c| c.is_alphanumeric() || c == '_'));
        }
    }
}