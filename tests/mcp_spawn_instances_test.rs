#[cfg(test)]
mod mcp_spawn_instances_tests {
    use tokio::sync::mpsc;
    use uuid::Uuid;
    
    // Mock the ClaudeMessage enum for MCP tool testing
    #[derive(Debug, Clone)]
    enum MockClaudeMessage {
        VedaSpawnInstances {
            instance_id: Uuid,
            task_description: String,
            num_instances: i32,
            working_dir: Option<String>,
        },
        VedaListInstances {
            instance_id: Uuid,
        },
        VedaCloseInstance {
            instance_id: Uuid,
            target_instance_name: String,
        },
        InternalCoordinateInstances {
            main_instance_id: Uuid,
            task_description: String,
            num_instances: usize,
            working_dir: String,
            is_ipc: bool,
        },
        StreamText {
            instance_id: Uuid,
            text: String,
            session_id: Option<String>,
        },
    }
    
    // Mock instance for testing
    #[derive(Debug, Clone)]
    struct MockInstance {
        id: Uuid,
        name: String,
        session_id: Option<String>,
        messages: Vec<(String, String)>, // (sender, content)
        working_directory: String,
        is_processing: bool,
    }
    
    impl MockInstance {
        fn new(name: String) -> Self {
            Self {
                id: Uuid::new_v4(),
                name,
                session_id: None,
                messages: Vec::new(),
                working_directory: "/home/user/project".to_string(),
                is_processing: false,
            }
        }
        
        fn add_message(&mut self, sender: String, content: String) {
            self.messages.push((sender, content));
        }
    }
    
    // Mock app to test MCP functionality
    struct MockApp {
        instances: Vec<MockInstance>,
        current_tab: usize,
        max_instances: usize,
        message_tx: mpsc::Sender<MockClaudeMessage>,
        message_rx: mpsc::Receiver<MockClaudeMessage>,
        coordination_in_progress: bool,
    }
    
    impl MockApp {
        fn new() -> Self {
            let (tx, rx) = mpsc::channel(100);
            let mut app = Self {
                instances: Vec::new(),
                current_tab: 0,
                max_instances: 5,
                message_tx: tx,
                message_rx: rx,
                coordination_in_progress: false,
            };
            // Create main instance
            app.instances.push(MockInstance::new("Veda-1".to_string()));
            app
        }
        
        async fn spawn_coordinated_instances_with_count(
            &mut self,
            main_instance_id: Uuid,
            task_description: &str,
            working_dir: &str,
            num_instances: usize,
        ) {
            // Simulate the spawning logic from main.rs
            let instances_to_spawn = (num_instances).min(self.max_instances - self.instances.len());
            
            // Add coordination message to main instance
            if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                main_instance.add_message(
                    "System".to_string(),
                    format!("🤝 Coordinating {} parallel instances for task division", instances_to_spawn)
                );
            }
            
            // Spawn additional instances
            for i in 0..instances_to_spawn {
                let instance_name = format!("Veda-{}", self.instances.len() + 1);
                let mut new_instance = MockInstance::new(instance_name.clone());
                new_instance.working_directory = working_dir.to_string();
                
                // Simulate assigning session ID (in real app this happens when Claude starts)
                new_instance.session_id = Some(format!("session-{}-{}", self.instances.len(), Uuid::new_v4()));
                
                // Add coordination message
                new_instance.add_message(
                    "System".to_string(),
                    format!(
                        "🤝 MULTI-INSTANCE COORDINATION MODE\n\nYOUR ASSIGNED SUBTASK: Part {} of {}\nTASK: {}\nWORKING DIRECTORY: {}",
                        i + 1, instances_to_spawn, task_description, working_dir
                    )
                );
                
                new_instance.is_processing = true; // Simulate auto-start
                self.instances.push(new_instance);
            }
            
            // Collect spawned names first to avoid borrow conflicts
            let spawned_names: Vec<String> = self.instances.iter()
                .skip(1)
                .map(|i| i.name.clone())
                .collect();
            
            // Update main instance with completion message
            if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
                main_instance.add_message(
                    "System".to_string(),
                    format!("✅ Spawned {} coordinated instances: {}", instances_to_spawn, spawned_names.join(", "))
                );
            }
        }
        
        async fn process_messages(&mut self) {
            while let Ok(msg) = self.message_rx.try_recv() {
                match msg {
                    MockClaudeMessage::VedaSpawnInstances { instance_id, task_description, num_instances, working_dir } => {
                        if self.coordination_in_progress {
                            if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                                instance.add_message("Tool".to_string(), 
                                    "❌ Cannot spawn instances: Coordination already in progress".to_string());
                            }
                            continue;
                        }
                        
                        self.coordination_in_progress = true;
                        
                        // Get working directory
                        let working_directory = working_dir.unwrap_or_else(|| {
                            self.instances.iter()
                                .find(|i| i.id == instance_id)
                                .map(|i| i.working_directory.clone())
                                .unwrap_or("/home/user/project".to_string())
                        });
                        
                        // Simulate sending to background analysis (in tests, we skip this)
                        self.message_tx.send(MockClaudeMessage::InternalCoordinateInstances {
                            main_instance_id: instance_id,
                            task_description: task_description.clone(),
                            num_instances: num_instances as usize,
                            working_dir: working_directory,
                            is_ipc: false,
                        }).await.unwrap();
                    }
                    MockClaudeMessage::VedaListInstances { instance_id } => {
                        let mut instance_info = Vec::new();
                        instance_info.push("📋 Current Claude Instances:".to_string());
                        
                        for (i, inst) in self.instances.iter().enumerate() {
                            let status = if inst.is_processing { "(Processing)" } else { "(Idle)" };
                            let current_marker = if i == self.current_tab { " ← Current" } else { "" };
                            instance_info.push(format!("  {}. {} {} - Dir: {}{}", 
                                i + 1, inst.name, status, inst.working_directory, current_marker));
                        }
                        
                        let message = instance_info.join("\n");
                        
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                            instance.add_message("Tool".to_string(), message);
                        }
                    }
                    MockClaudeMessage::VedaCloseInstance { instance_id, target_instance_name } => {
                        let target_index = self.instances.iter().position(|inst| inst.name == target_instance_name);
                        let instances_len = self.instances.len();
                        
                        let result_message = if let Some(target_idx) = target_index {
                            if target_idx == 0 {
                                "❌ Cannot close the main instance (Veda-1)".to_string()
                            } else if instances_len <= 1 {
                                "❌ Cannot close the last remaining instance".to_string()
                            } else {
                                let closed_name = self.instances[target_idx].name.clone();
                                self.instances.remove(target_idx);
                                
                                // Adjust current tab if necessary
                                if self.current_tab >= self.instances.len() {
                                    self.current_tab = self.instances.len() - 1;
                                } else if self.current_tab > target_idx {
                                    self.current_tab -= 1;
                                }
                                
                                format!("✅ Closed instance: {}", closed_name)
                            }
                        } else {
                            format!("❌ Instance '{}' not found", target_instance_name)
                        };
                        
                        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                            instance.add_message("Tool".to_string(), result_message);
                        }
                    }
                    MockClaudeMessage::InternalCoordinateInstances { main_instance_id, task_description, num_instances, working_dir, .. } => {
                        self.spawn_coordinated_instances_with_count(main_instance_id, &task_description, &working_dir, num_instances).await;
                        self.coordination_in_progress = false;
                    }
                    _ => {}
                }
            }
        }
    }
    
    #[tokio::test]
    async fn test_spawn_instances_tool() {
        let mut app = MockApp::new();
        let main_id = app.instances[0].id;
        
        // Test spawning instances
        app.message_tx.send(MockClaudeMessage::VedaSpawnInstances {
            instance_id: main_id,
            task_description: "Build a REST API with authentication".to_string(),
            num_instances: 3,
            working_dir: Some("/project/api".to_string()),
        }).await.unwrap();
        
        // Process the spawn request
        app.process_messages().await;
        app.process_messages().await; // Process the internal coordination message
        
        // Verify instances were spawned
        assert_eq!(app.instances.len(), 4); // 1 main + 3 spawned
        assert_eq!(app.instances[0].name, "Veda-1");
        assert_eq!(app.instances[1].name, "Veda-2");
        assert_eq!(app.instances[2].name, "Veda-3");
        assert_eq!(app.instances[3].name, "Veda-4");
        
        // Verify working directories
        for i in 1..4 {
            assert_eq!(app.instances[i].working_directory, "/project/api");
        }
        
        // Verify session IDs were assigned
        assert!(app.instances[0].session_id.is_none()); // Main has no session
        for i in 1..4 {
            assert!(app.instances[i].session_id.is_some());
            assert!(app.instances[i].session_id.as_ref().unwrap().starts_with("session-"));
        }
        
        // Verify coordination messages
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "System" && c.contains("Coordinating 3 parallel instances")));
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "System" && c.contains("Spawned 3 coordinated instances")));
        
        for i in 1..4 {
            assert!(app.instances[i].messages.iter().any(|(s, c)| 
                s == "System" && c.contains("MULTI-INSTANCE COORDINATION MODE")));
        }
    }
    
    #[tokio::test]
    async fn test_list_instances_tool() {
        let mut app = MockApp::new();
        let main_id = app.instances[0].id;
        
        // Spawn some instances first
        app.spawn_coordinated_instances_with_count(main_id, "Test task", "/project", 2).await;
        
        // Set processing states
        app.instances[1].is_processing = true;
        app.instances[2].is_processing = false;
        app.current_tab = 1;
        
        // Test listing instances
        app.message_tx.send(MockClaudeMessage::VedaListInstances {
            instance_id: main_id,
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Verify list output
        let tool_messages: Vec<_> = app.instances[0].messages.iter()
            .filter(|(s, _)| s == "Tool")
            .collect();
        
        assert_eq!(tool_messages.len(), 1);
        let list_output = &tool_messages[0].1;
        
        assert!(list_output.contains("📋 Current Claude Instances:"));
        assert!(list_output.contains("1. Veda-1 (Idle)"));
        assert!(list_output.contains("2. Veda-2 (Processing)"));
        assert!(list_output.contains("← Current")); // Tab 2 is current
        assert!(list_output.contains("3. Veda-3 (Idle)"));
    }
    
    #[tokio::test]
    async fn test_close_instance_tool() {
        let mut app = MockApp::new();
        let main_id = app.instances[0].id;
        
        // Spawn instances
        app.spawn_coordinated_instances_with_count(main_id, "Test task", "/project", 3).await;
        
        // Test closing a valid instance
        app.message_tx.send(MockClaudeMessage::VedaCloseInstance {
            instance_id: main_id,
            target_instance_name: "Veda-3".to_string(),
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Verify instance was closed
        assert_eq!(app.instances.len(), 3); // 1 main + 2 remaining
        assert!(!app.instances.iter().any(|i| i.name == "Veda-3"));
        
        // Verify success message
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "Tool" && c == "✅ Closed instance: Veda-3"));
        
        // Test closing main instance (should fail)
        app.message_tx.send(MockClaudeMessage::VedaCloseInstance {
            instance_id: main_id,
            target_instance_name: "Veda-1".to_string(),
        }).await.unwrap();
        
        app.process_messages().await;
        
        assert_eq!(app.instances.len(), 3); // No change
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "Tool" && c == "❌ Cannot close the main instance (Veda-1)"));
        
        // Test closing non-existent instance
        app.message_tx.send(MockClaudeMessage::VedaCloseInstance {
            instance_id: main_id,
            target_instance_name: "Veda-99".to_string(),
        }).await.unwrap();
        
        app.process_messages().await;
        
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "Tool" && c == "❌ Instance 'Veda-99' not found"));
    }
    
    #[tokio::test]
    async fn test_max_instances_limit() {
        let mut app = MockApp::new();
        app.max_instances = 3; // Lower limit for testing
        let main_id = app.instances[0].id;
        
        // Try to spawn more than max
        app.message_tx.send(MockClaudeMessage::VedaSpawnInstances {
            instance_id: main_id,
            task_description: "Test task".to_string(),
            num_instances: 5,
            working_dir: None,
        }).await.unwrap();
        
        app.process_messages().await;
        app.process_messages().await;
        
        // Should only spawn up to max
        assert_eq!(app.instances.len(), 3); // Hit the max
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "System" && c.contains("Spawned 2 coordinated instances"))); // Only 2 were spawned
    }
    
    #[tokio::test]
    async fn test_coordination_in_progress_protection() {
        let mut app = MockApp::new();
        let main_id = app.instances[0].id;
        
        // Set coordination in progress
        app.coordination_in_progress = true;
        
        // Try to spawn while coordination is in progress
        app.message_tx.send(MockClaudeMessage::VedaSpawnInstances {
            instance_id: main_id,
            task_description: "Another task".to_string(),
            num_instances: 2,
            working_dir: None,
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Should reject the request
        assert_eq!(app.instances.len(), 1); // No new instances
        assert!(app.instances[0].messages.iter().any(|(s, c)| 
            s == "Tool" && c.contains("Cannot spawn instances: Coordination already in progress")));
    }
    
    #[tokio::test]
    async fn test_working_directory_inheritance() {
        let mut app = MockApp::new();
        let main_id = app.instances[0].id;
        
        // Set main instance working directory
        app.instances[0].working_directory = "/custom/path".to_string();
        
        // Spawn without specifying working_dir
        app.message_tx.send(MockClaudeMessage::VedaSpawnInstances {
            instance_id: main_id,
            task_description: "Test task".to_string(),
            num_instances: 2,
            working_dir: None, // Should inherit from main
        }).await.unwrap();
        
        app.process_messages().await;
        app.process_messages().await;
        
        // Verify inherited working directory
        assert_eq!(app.instances[1].working_directory, "/custom/path");
        assert_eq!(app.instances[2].working_directory, "/custom/path");
    }
    
    #[tokio::test]
    async fn test_tab_adjustment_after_close() {
        let mut app = MockApp::new();
        let main_id = app.instances[0].id;
        
        // Spawn instances and set current tab
        app.spawn_coordinated_instances_with_count(main_id, "Test", "/project", 3).await;
        app.current_tab = 3; // Viewing Veda-4
        
        // Close Veda-2
        app.message_tx.send(MockClaudeMessage::VedaCloseInstance {
            instance_id: main_id,
            target_instance_name: "Veda-2".to_string(),
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Current tab should adjust down
        assert_eq!(app.current_tab, 2); // Was 3, now 2
        
        // Close current tab
        app.message_tx.send(MockClaudeMessage::VedaCloseInstance {
            instance_id: main_id,
            target_instance_name: "Veda-4".to_string(),
        }).await.unwrap();
        
        app.process_messages().await;
        
        // Should move to last valid tab
        assert_eq!(app.current_tab, 1); // Now viewing Veda-3
    }
}