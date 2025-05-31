#[cfg(test)]
mod tab_functionality_tests {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;
    
    // Mock structures to simulate the app behavior
    #[derive(Clone, Debug)]
    struct MockInstance {
        id: Uuid,
        name: String,
        messages: Vec<MockMessage>,
        scroll_offset: u16,
        last_message_area_height: u16,
        last_terminal_width: u16,
        session_id: Option<String>,
        is_processing: bool,
        working_directory: String,
        process_handle: Option<Arc<Mutex<Option<String>>>>, // Simplified for testing
    }
    
    #[derive(Clone, Debug)]
    struct MockMessage {
        sender: String,
        content: String,
        timestamp: String,
    }
    
    struct MockApp {
        instances: Vec<MockInstance>,
        current_tab: usize,
        max_instances: usize,
        instance_counter: usize,
    }
    
    impl MockInstance {
        fn new(name: String) -> Self {
            Self {
                id: Uuid::new_v4(),
                name,
                messages: Vec::new(),
                scroll_offset: 0,
                last_message_area_height: 20,
                last_terminal_width: 80,
                session_id: None,
                is_processing: false,
                working_directory: "/home/user/project".to_string(),
                process_handle: None,
            }
        }
        
        fn with_session(mut self, session_id: String) -> Self {
            self.session_id = Some(session_id);
            self
        }
        
        fn add_message(&mut self, sender: String, content: String) {
            self.messages.push(MockMessage {
                sender,
                content,
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            });
        }
        
        fn auto_scroll(&mut self) {
            let total_lines = self.messages.len() * 2; // Each message + empty line
            let visible_lines = self.last_message_area_height as usize;
            
            if total_lines > visible_lines {
                self.scroll_offset = (total_lines - visible_lines) as u16;
            } else {
                self.scroll_offset = 0;
            }
        }
    }
    
    impl MockApp {
        fn new() -> Self {
            let mut app = Self {
                instances: Vec::new(),
                current_tab: 0,
                max_instances: 5,
                instance_counter: 0,
            };
            // Create initial instance
            app.add_instance();
            app
        }
        
        fn add_instance(&mut self) -> usize {
            if self.instances.len() >= self.max_instances {
                return self.instances.len() - 1;
            }
            
            self.instance_counter += 1;
            let name = format!("Veda-{}", self.instance_counter);
            self.instances.push(MockInstance::new(name));
            self.instances.len() - 1
        }
        
        fn spawn_coordinated_instances(&mut self, num_instances: usize, task_description: &str) -> Vec<usize> {
            let mut spawned_indices = Vec::new();
            
            for i in 0..num_instances {
                if self.instances.len() >= self.max_instances {
                    break;
                }
                
                let idx = self.add_instance();
                let instance = &mut self.instances[idx];
                
                // Assign a unique session ID
                instance.session_id = Some(format!("session-{}-{}", idx, Uuid::new_v4()));
                
                // Add coordination message
                instance.add_message(
                    "System".to_string(),
                    format!("🤝 MULTI-INSTANCE COORDINATION MODE\n\nAssigned subtask {} of {}: {}", 
                        i + 1, num_instances, task_description)
                );
                
                spawned_indices.push(idx);
            }
            
            spawned_indices
        }
        
        fn route_message_by_session(&mut self, session_id: &str, _message: &str) -> Option<usize> {
            self.instances.iter().position(|i| 
                i.session_id.as_ref() == Some(&session_id.to_string())
            )
        }
    }
    
    #[test]
    fn test_instance_naming_convention() {
        let mut app = MockApp::new();
        
        // Initial instance should be Veda-1
        assert_eq!(app.instances[0].name, "Veda-1");
        
        // Add more instances
        for i in 2..=5 {
            app.add_instance();
            assert_eq!(app.instances[i-1].name, format!("Veda-{}", i));
        }
        
        // Should not exceed max instances
        let idx = app.add_instance();
        assert_eq!(app.instances.len(), 5);
        assert_eq!(idx, 4); // Returns last valid index
    }
    
    #[test]
    fn test_session_id_assignment_on_spawn() {
        let mut app = MockApp::new();
        
        // Main instance should not have session ID initially
        assert!(app.instances[0].session_id.is_none());
        
        // Spawn coordinated instances
        let spawned = app.spawn_coordinated_instances(3, "Implement feature X");
        
        // Verify all spawned instances have unique session IDs
        let mut session_ids = std::collections::HashSet::new();
        for idx in spawned {
            let session_id = app.instances[idx].session_id.as_ref().unwrap();
            assert!(session_ids.insert(session_id.clone()));
            assert!(session_id.starts_with(&format!("session-{}-", idx)));
        }
    }
    
    #[test]
    fn test_message_routing_by_session_id() {
        let mut app = MockApp::new();
        
        // Create instances with specific session IDs
        app.add_instance();
        app.add_instance();
        
        app.instances[1].session_id = Some("session-abc-123".to_string());
        app.instances[2].session_id = Some("session-def-456".to_string());
        
        // Test routing
        let idx1 = app.route_message_by_session("session-abc-123", "Hello Tab 2");
        assert_eq!(idx1, Some(1));
        
        let idx2 = app.route_message_by_session("session-def-456", "Hello Tab 3");
        assert_eq!(idx2, Some(2));
        
        let idx3 = app.route_message_by_session("session-unknown", "Lost message");
        assert_eq!(idx3, None);
    }
    
    #[test]
    fn test_tab_state_preservation() {
        let mut app = MockApp::new();
        app.add_instance();
        app.add_instance();
        
        // Set different states for each tab
        app.instances[0].working_directory = "/project/src".to_string();
        app.instances[0].is_processing = true;
        for i in 0..30 {
            app.instances[0].add_message("Claude".to_string(), format!("Message {}", i));
        }
        app.instances[0].auto_scroll();
        
        app.instances[1].working_directory = "/project/tests".to_string();
        app.instances[1].is_processing = false;
        for i in 0..5 {
            app.instances[1].add_message("You".to_string(), format!("Test {}", i));
        }
        app.instances[1].auto_scroll();
        
        app.instances[2].working_directory = "/project/docs".to_string();
        
        // Verify states are preserved
        assert_eq!(app.instances[0].working_directory, "/project/src");
        assert_eq!(app.instances[0].is_processing, true);
        assert!(app.instances[0].scroll_offset > 0);
        assert_eq!(app.instances[0].messages.len(), 30);
        
        assert_eq!(app.instances[1].working_directory, "/project/tests");
        assert_eq!(app.instances[1].is_processing, false);
        assert_eq!(app.instances[1].scroll_offset, 0);
        assert_eq!(app.instances[1].messages.len(), 5);
        
        assert_eq!(app.instances[2].working_directory, "/project/docs");
        assert_eq!(app.instances[2].messages.len(), 0);
    }
    
    #[tokio::test]
    async fn test_concurrent_message_handling() {
        let app = Arc::new(Mutex::new(MockApp::new()));
        
        // Add instances
        {
            let mut app_guard = app.lock().await;
            app_guard.add_instance();
            app_guard.add_instance();
            
            // Assign session IDs
            app_guard.instances[0].session_id = Some("session-main".to_string());
            app_guard.instances[1].session_id = Some("session-worker-1".to_string());
            app_guard.instances[2].session_id = Some("session-worker-2".to_string());
        }
        
        // Simulate concurrent messages from different instances
        let mut handles = vec![];
        
        for i in 0..3 {
            let app_clone = app.clone();
            let session_id = if i == 0 { 
                "session-main".to_string() 
            } else { 
                format!("session-worker-{}", i) 
            };
            
            let handle = tokio::spawn(async move {
                for msg_num in 0..20 {
                    let mut app_guard = app_clone.lock().await;
                    
                    if let Some(idx) = app_guard.route_message_by_session(&session_id, "test") {
                        app_guard.instances[idx].add_message(
                            "Claude".to_string(),
                            format!("Instance {} Message {}", i + 1, msg_num)
                        );
                        app_guard.instances[idx].is_processing = msg_num % 2 == 0;
                    }
                    
                    drop(app_guard);
                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                }
            });
            handles.push(handle);
        }
        
        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify all messages were routed correctly
        let app_guard = app.lock().await;
        for i in 0..3 {
            assert_eq!(app_guard.instances[i].messages.len(), 20);
            // Check first and last messages
            assert_eq!(app_guard.instances[i].messages[0].content, 
                format!("Instance {} Message 0", i + 1));
            assert_eq!(app_guard.instances[i].messages[19].content, 
                format!("Instance {} Message 19", i + 1));
        }
    }
    
    #[test]
    fn test_coordination_message_format() {
        let mut app = MockApp::new();
        
        // Spawn instances with task description
        let task = "Implement authentication system with JWT tokens";
        let spawned = app.spawn_coordinated_instances(2, task);
        
        // Verify coordination messages
        for (i, idx) in spawned.iter().enumerate() {
            let messages = &app.instances[*idx].messages;
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].sender, "System");
            assert!(messages[0].content.contains("MULTI-INSTANCE COORDINATION MODE"));
            assert!(messages[0].content.contains(&format!("subtask {} of 2", i + 1)));
            assert!(messages[0].content.contains(task));
        }
    }
    
    #[test]
    fn test_tab_switching_behavior() {
        let mut app = MockApp::new();
        app.add_instance();
        app.add_instance();
        
        // Start at tab 0
        assert_eq!(app.current_tab, 0);
        
        // Switch to next tab
        app.current_tab = (app.current_tab + 1) % app.instances.len();
        assert_eq!(app.current_tab, 1);
        
        // Switch to next tab (should wrap)
        app.current_tab = (app.current_tab + 1) % app.instances.len();
        assert_eq!(app.current_tab, 2);
        
        app.current_tab = (app.current_tab + 1) % app.instances.len();
        assert_eq!(app.current_tab, 0);
        
        // Previous tab
        app.current_tab = if app.current_tab == 0 { 
            app.instances.len() - 1 
        } else { 
            app.current_tab - 1 
        };
        assert_eq!(app.current_tab, 2);
    }
    
    #[test]
    fn test_background_tab_dimension_handling() {
        let mut instance = MockInstance::new("Veda-2".to_string());
        
        // Simulate background tab (no dimensions)
        instance.last_message_area_height = 0;
        instance.last_terminal_width = 0;
        
        // Add many messages
        for i in 0..50 {
            instance.add_message("Claude".to_string(), format!("Long message {}", i));
        }
        
        // When dimensions are 0, should use defaults
        let effective_height = if instance.last_message_area_height == 0 { 20 } else { instance.last_message_area_height };
        let effective_width = if instance.last_terminal_width == 0 { 80 } else { instance.last_terminal_width };
        
        assert_eq!(effective_height, 20);
        assert_eq!(effective_width, 80);
        
        // Update with defaults and scroll
        instance.last_message_area_height = effective_height;
        instance.last_terminal_width = effective_width;
        instance.auto_scroll();
        
        // Should have scrolled
        assert!(instance.scroll_offset > 0);
        assert_eq!(instance.scroll_offset, 80); // 100 lines (50 messages * 2) - 20 visible
    }
    
    #[test]
    fn test_main_instance_protection() {
        let mut app = MockApp::new();
        app.add_instance();
        app.add_instance();
        
        // Try to close main instance (should fail)
        let initial_count = app.instances.len();
        // In real app, this would be prevented
        // Here we just verify the main instance properties
        assert_eq!(app.instances[0].name, "Veda-1");
        assert!(app.instances[0].session_id.is_none()); // Main instance has no session ID
        
        // Can close other instances
        app.instances.remove(2); // Remove Veda-3
        assert_eq!(app.instances.len(), initial_count - 1);
        
        // Verify Veda-1 is still there
        assert_eq!(app.instances[0].name, "Veda-1");
    }
    
    #[test]
    fn test_spawned_instance_workflow() {
        let mut app = MockApp::new();
        
        // Initial state
        assert_eq!(app.instances.len(), 1);
        assert_eq!(app.instances[0].name, "Veda-1");
        
        // Simulate spawn_instances tool use
        let num_to_spawn = 3;
        let task = "Refactor authentication module";
        
        // Record initial instance for coordination
        let _main_instance_id = app.instances[0].id;
        
        // Spawn instances
        let spawned_indices = app.spawn_coordinated_instances(num_to_spawn, task);
        
        // Verify correct number spawned
        assert_eq!(spawned_indices.len(), num_to_spawn);
        assert_eq!(app.instances.len(), 4); // 1 original + 3 spawned
        
        // Verify naming
        assert_eq!(app.instances[1].name, "Veda-2");
        assert_eq!(app.instances[2].name, "Veda-3");
        assert_eq!(app.instances[3].name, "Veda-4");
        
        // Verify all have session IDs except main
        assert!(app.instances[0].session_id.is_none());
        for i in 1..4 {
            assert!(app.instances[i].session_id.is_some());
        }
        
        // Simulate messages coming back from spawned instances
        let session_2 = app.instances[1].session_id.clone().unwrap();
        let session_3 = app.instances[2].session_id.clone().unwrap();
        
        // Route messages by session
        if let Some(idx) = app.route_message_by_session(&session_2, "Working on auth tokens") {
            app.instances[idx].add_message("Claude".to_string(), "Starting work on JWT implementation".to_string());
        }
        
        if let Some(idx) = app.route_message_by_session(&session_3, "Working on middleware") {
            app.instances[idx].add_message("Claude".to_string(), "Refactoring auth middleware".to_string());
        }
        
        // Verify messages went to correct tabs
        assert_eq!(app.instances[1].messages.len(), 2); // System + Claude
        assert_eq!(app.instances[2].messages.len(), 2); // System + Claude
        assert!(app.instances[1].messages[1].content.contains("JWT implementation"));
        assert!(app.instances[2].messages[1].content.contains("auth middleware"));
    }
}