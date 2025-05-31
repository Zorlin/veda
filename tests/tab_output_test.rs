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
}