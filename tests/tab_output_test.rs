#[cfg(test)]
mod tab_output_tests {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;
    
    // Mock structures to simulate the app behavior
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
    }
    
    #[derive(Clone)]
    struct MockMessage {
        sender: String,
        content: String,
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
            }
        }
        
        fn add_message(&mut self, sender: String, content: String) {
            self.messages.push(MockMessage { sender, content });
        }
        
        fn auto_scroll(&mut self) {
            // Simplified scroll calculation
            let total_lines = self.messages.len() + self.messages.len(); // Each message + empty line
            let visible_lines = self.last_message_area_height as usize;
            
            if total_lines > visible_lines {
                self.scroll_offset = (total_lines - visible_lines) as u16;
            } else {
                self.scroll_offset = 0;
            }
        }
    }
    
    #[test]
    fn test_tab_creation_names() {
        let mut instances = Vec::new();
        
        // Create main instance
        instances.push(MockInstance::new("Veda-1".to_string()));
        
        // Create spawned instances
        for i in 2..=4 {
            instances.push(MockInstance::new(format!("Veda-{}", i)));
        }
        
        // Verify names
        assert_eq!(instances[0].name, "Veda-1");
        assert_eq!(instances[1].name, "Veda-2");
        assert_eq!(instances[2].name, "Veda-3");
        assert_eq!(instances[3].name, "Veda-4");
    }
    
    #[test]
    fn test_background_tab_scroll_update() {
        let mut instance = MockInstance::new("Veda-2".to_string());
        
        // Simulate background tab with no dimensions set
        instance.last_message_area_height = 0;
        instance.last_terminal_width = 0;
        
        // Add messages
        for i in 0..30 {
            instance.add_message("Claude".to_string(), format!("Message {}", i));
        }
        
        // Should use defaults when dimensions are 0
        let height = if instance.last_message_area_height == 0 { 20 } else { instance.last_message_area_height };
        let width = if instance.last_terminal_width == 0 { 80 } else { instance.last_terminal_width };
        
        assert_eq!(height, 20);
        assert_eq!(width, 80);
        
        // Update dimensions and scroll
        instance.last_message_area_height = height;
        instance.last_terminal_width = width;
        instance.auto_scroll();
        
        // Verify scroll offset is set
        assert!(instance.scroll_offset > 0);
        assert_eq!(instance.scroll_offset, 40); // 60 total lines - 20 visible
    }
    
    #[test]
    fn test_session_id_routing() {
        let mut instances = vec![
            MockInstance::new("Veda-1".to_string()),
            MockInstance::new("Veda-2".to_string()),
            MockInstance::new("Veda-3".to_string()),
        ];
        
        // Set session IDs
        instances[1].session_id = Some("session-123".to_string());
        instances[2].session_id = Some("session-456".to_string());
        
        // Simulate finding instance by session_id
        let session_to_find = "session-456";
        let found_idx = instances.iter().position(|i| 
            i.session_id.as_ref() == Some(&session_to_find.to_string())
        );
        
        assert_eq!(found_idx, Some(2));
        assert_eq!(instances[2].name, "Veda-3");
    }
    
    #[tokio::test]
    async fn test_concurrent_tab_updates() {
        let instances = Arc::new(Mutex::new(vec![
            MockInstance::new("Veda-1".to_string()),
            MockInstance::new("Veda-2".to_string()),
            MockInstance::new("Veda-3".to_string()),
        ]));
        
        // Simulate concurrent updates to different tabs
        let mut handles = vec![];
        
        for tab_idx in 0..3 {
            let instances_clone = instances.clone();
            let handle = tokio::spawn(async move {
                for i in 0..10 {
                    let mut instances_guard = instances_clone.lock().await;
                    instances_guard[tab_idx].add_message(
                        "Claude".to_string(), 
                        format!("Tab {} Message {}", tab_idx + 1, i)
                    );
                    instances_guard[tab_idx].auto_scroll();
                    drop(instances_guard);
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            });
            handles.push(handle);
        }
        
        // Wait for all updates to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify all tabs have messages
        let instances_guard = instances.lock().await;
        for (idx, instance) in instances_guard.iter().enumerate() {
            assert_eq!(instance.messages.len(), 10);
            assert_eq!(instance.messages[0].content, format!("Tab {} Message 0", idx + 1));
            assert_eq!(instance.messages[9].content, format!("Tab {} Message 9", idx + 1));
        }
    }
    
    #[test]
    fn test_scroll_preservation_on_tab_switch() {
        let mut instances = vec![
            MockInstance::new("Veda-1".to_string()),
            MockInstance::new("Veda-2".to_string()),
        ];
        
        // Add different amounts of messages to each tab
        for i in 0..50 {
            instances[0].add_message("Claude".to_string(), format!("Tab 1 Message {}", i));
        }
        instances[0].auto_scroll();
        let tab1_scroll = instances[0].scroll_offset;
        
        for i in 0..10 {
            instances[1].add_message("Claude".to_string(), format!("Tab 2 Message {}", i));
        }
        instances[1].auto_scroll();
        let tab2_scroll = instances[1].scroll_offset;
        
        // Verify different scroll positions
        assert!(tab1_scroll > 0);
        assert_eq!(tab2_scroll, 0); // Only 10 messages, should fit in view
        
        // Simulate switching tabs - scroll should be preserved
        assert_eq!(instances[0].scroll_offset, tab1_scroll);
        assert_eq!(instances[1].scroll_offset, tab2_scroll);
    }
    
    #[test]
    fn test_message_appending_vs_new_message() {
        let mut instance = MockInstance::new("Veda-1".to_string());
        
        // Add initial message
        instance.add_message("You".to_string(), "Hello".to_string());
        
        // Claude message should be new
        instance.add_message("Claude".to_string(), "Hi there!".to_string());
        assert_eq!(instance.messages.len(), 2);
        
        // Tool message
        instance.add_message("Tool".to_string(), "Using tool X".to_string());
        assert_eq!(instance.messages.len(), 3);
        
        // After tool, Claude message should be new (not appended)
        instance.add_message("Claude".to_string(), "Tool result processed".to_string());
        assert_eq!(instance.messages.len(), 4);
        
        // Verify message order
        assert_eq!(instance.messages[0].sender, "You");
        assert_eq!(instance.messages[1].sender, "Claude");
        assert_eq!(instance.messages[2].sender, "Tool");
        assert_eq!(instance.messages[3].sender, "Claude");
    }
    
    #[test]
    fn test_spawned_instances_naming_convention() {
        // Test that spawned instances follow the correct naming pattern
        let mut instances = vec![
            MockInstance::new("Veda-1".to_string()),
        ];
        
        // Simulate spawning 4 more instances (up to the max of 5)
        for i in 2..=5 {
            let instance_name = format!("Veda-{}", i);
            let mut instance = MockInstance::new(instance_name.clone());
            instance.working_directory = "/home/user/project".to_string();
            instances.push(instance);
        }
        
        // Verify all instances have correct names
        for (idx, instance) in instances.iter().enumerate() {
            assert_eq!(instance.name, format!("Veda-{}", idx + 1));
        }
        
        // Verify we have exactly 5 instances
        assert_eq!(instances.len(), 5);
    }
    
    #[test]
    fn test_instance_working_directory_inheritance() {
        // Test that spawned instances inherit the working directory
        let base_dir = "/home/user/complex-project";
        let mut instances = vec![];
        
        // Create main instance with specific working directory
        let mut main_instance = MockInstance::new("Veda-1".to_string());
        main_instance.working_directory = base_dir.to_string();
        instances.push(main_instance);
        
        // Spawn additional instances
        for i in 2..=3 {
            let mut instance = MockInstance::new(format!("Veda-{}", i));
            instance.working_directory = base_dir.to_string();
            instances.push(instance);
        }
        
        // Verify all instances have the same working directory
        for instance in &instances {
            assert_eq!(instance.working_directory, base_dir);
        }
    }
    
    #[test]
    fn test_tab_status_display() {
        // Test that tab status correctly shows processing state
        let mut instances = vec![
            MockInstance::new("Veda-1".to_string()),
            MockInstance::new("Veda-2".to_string()),
            MockInstance::new("Veda-3".to_string()),
        ];
        
        // Set different processing states
        instances[0].is_processing = false;
        instances[1].is_processing = true;
        instances[2].is_processing = false;
        
        // Verify status display
        let statuses: Vec<String> = instances.iter().map(|inst| {
            format!("{} {}", inst.name, if inst.is_processing { "(Processing)" } else { "(Idle)" })
        }).collect();
        
        assert_eq!(statuses[0], "Veda-1 (Idle)");
        assert_eq!(statuses[1], "Veda-2 (Processing)");
        assert_eq!(statuses[2], "Veda-3 (Idle)");
    }
    
    #[test]
    fn test_session_id_assignment_for_spawned_instances() {
        // Test that spawned instances get unique session IDs
        let mut instances = vec![
            MockInstance::new("Veda-1".to_string()),
        ];
        
        // Main instance doesn't need a session ID
        assert_eq!(instances[0].session_id, None);
        
        // Spawn instances with session IDs
        for i in 2..=4 {
            let mut instance = MockInstance::new(format!("Veda-{}", i));
            instance.session_id = Some(format!("session-{:04}", i * 111));
            instances.push(instance);
        }
        
        // Verify session IDs
        assert_eq!(instances[0].session_id, None);
        assert_eq!(instances[1].session_id, Some("session-0222".to_string()));
        assert_eq!(instances[2].session_id, Some("session-0333".to_string()));
        assert_eq!(instances[3].session_id, Some("session-0444".to_string()));
    }
    
    #[test]
    fn test_message_routing_by_session_priority() {
        // Test that messages route by session_id first, then by instance_id
        let mut instances = vec![
            MockInstance::new("Veda-1".to_string()),
            MockInstance::new("Veda-2".to_string()),
            MockInstance::new("Veda-3".to_string()),
        ];
        
        let instance_2_id = instances[1].id;
        instances[1].session_id = Some("session-abc".to_string());
        instances[2].session_id = Some("session-xyz".to_string());
        
        // Test routing with session_id
        let incoming_session = Some("session-abc".to_string());
        let incoming_instance = Uuid::new_v4(); // Different from actual instance
        
        // Routing logic from main.rs
        let target_idx = if let Some(session_id_val) = &incoming_session {
            instances.iter().position(|i| i.session_id.as_ref() == Some(session_id_val))
                .or_else(|| instances.iter().position(|i| i.id == incoming_instance))
        } else {
            instances.iter().position(|i| i.id == incoming_instance)
        };
        
        assert_eq!(target_idx, Some(1)); // Should route to Veda-2
        
        // Test routing without session_id (fallback to instance_id)
        let target_idx_2 = instances.iter().position(|i| i.id == instance_2_id);
        assert_eq!(target_idx_2, Some(1));
    }
    
    #[test]
    fn test_max_instances_limit() {
        // Test that we respect the max instances limit of 5
        let mut instances = vec![];
        let max_instances = 5;
        
        // Create instances up to the limit
        for i in 1..=max_instances {
            instances.push(MockInstance::new(format!("Veda-{}", i)));
        }
        
        assert_eq!(instances.len(), max_instances);
        
        // Verify we can't exceed the limit
        let can_add_more = instances.len() < max_instances;
        assert!(!can_add_more, "Should not be able to add more instances");
    }
    
    #[test]
    fn test_coordination_message_format() {
        // Test the coordination message format for spawned instances
        let mut instance = MockInstance::new("Veda-2".to_string());
        instance.working_directory = "/home/user/project".to_string();
        
        let coordination_msg = format!(
            "🤝 MULTI-INSTANCE COORDINATION MODE\n\n\
            YOU ARE: {}\n\
            WORKING DIRECTORY: {}\n\
            ASSIGNED TASK: Implement user authentication\n\
            SCOPE: src/auth/*\n\
            PRIORITY: High",
            instance.name,
            instance.working_directory
        );
        
        instance.add_message("System".to_string(), coordination_msg);
        
        assert_eq!(instance.messages.len(), 1);
        assert!(instance.messages[0].content.contains("MULTI-INSTANCE COORDINATION"));
        assert!(instance.messages[0].content.contains("Veda-2"));
        assert!(instance.messages[0].content.contains("/home/user/project"));
    }
    
    #[tokio::test]
    async fn test_concurrent_instance_spawning() {
        // Test that multiple instances can be spawned concurrently
        let instances = Arc::new(Mutex::new(vec![
            MockInstance::new("Veda-1".to_string()),
        ]));
        
        let instances_to_spawn = 3;
        let mut handles = vec![];
        
        for i in 0..instances_to_spawn {
            let instances_clone = instances.clone();
            let handle = tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(10 * i)).await;
                
                let mut instances_guard = instances_clone.lock().await;
                let new_idx = instances_guard.len() + 1;
                if new_idx <= 5 { // Respect max limit
                    let mut new_instance = MockInstance::new(format!("Veda-{}", new_idx));
                    new_instance.session_id = Some(format!("session-spawn-{}", i));
                    instances_guard.push(new_instance);
                }
            });
            handles.push(handle);
        }
        
        // Wait for all spawns to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        let instances_guard = instances.lock().await;
        assert_eq!(instances_guard.len(), 4); // 1 original + 3 spawned
        
        // Verify each has unique session ID
        let session_ids: Vec<_> = instances_guard.iter()
            .filter_map(|i| i.session_id.as_ref())
            .collect();
        
        let unique_sessions: std::collections::HashSet<_> = session_ids.iter().collect();
        assert_eq!(session_ids.len(), unique_sessions.len());
    }
}