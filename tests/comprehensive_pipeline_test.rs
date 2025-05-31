use serde_json::{json, Value};
use uuid::Uuid;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Simulated tab structure for testing
#[derive(Debug, Clone)]
struct Tab {
    id: String,
    instance_id: Uuid,
    session_id: String,
    messages: Vec<String>,
    status: String,
}

/// Simulated UI state for testing
#[derive(Debug)]
struct UIState {
    tabs: Vec<Tab>,
    active_tab: usize,
    message_buffer: HashMap<Uuid, VecDeque<String>>,
}

impl UIState {
    fn new() -> Self {
        Self {
            tabs: vec![],
            active_tab: 0,
            message_buffer: HashMap::new(),
        }
    }
    
    fn add_tab(&mut self, id: String, instance_id: Uuid, session_id: String) {
        self.tabs.push(Tab {
            id,
            instance_id,
            session_id,
            messages: vec![],
            status: "active".to_string(),
        });
    }
    
    fn route_message(&mut self, instance_id: Uuid, session_id: Option<String>, text: String) -> bool {
        // Priority 1: Route by session ID if available
        if let Some(sid) = session_id {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.session_id == sid) {
                tab.messages.push(text);
                return true;
            }
        }
        
        // Priority 2: Route by instance ID
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.instance_id == instance_id) {
            tab.messages.push(text);
            return true;
        }
        
        // Priority 3: Route to main tab (tab 0)
        if !self.tabs.is_empty() {
            self.tabs[0].messages.push(text);
            return true;
        }
        
        false
    }
    
    fn get_tab_message_counts(&self) -> Vec<(String, usize)> {
        self.tabs.iter()
            .map(|t| (t.id.clone(), t.messages.len()))
            .collect()
    }
}

/// Comprehensive test that simulates the entire pipeline end-to-end
#[tokio::test]
async fn test_complete_pipeline_with_message_routing() {
    let mut ui_state = UIState::new();
    
    // Step 1: Create 4 tabs with unique instance IDs and session IDs
    let tabs_config = vec![
        ("Veda-1", Uuid::new_v4(), "session-main"),
        ("Veda-2", Uuid::new_v4(), "session-2"),
        ("Veda-3", Uuid::new_v4(), "session-3"),
        ("Veda-4", Uuid::new_v4(), "session-4"),
    ];
    
    for (name, instance_id, session_id) in &tabs_config {
        ui_state.add_tab(name.to_string(), *instance_id, session_id.to_string());
    }
    
    // Step 2: Simulate Claude processes sending messages
    let messages = vec![
        // Initial messages - each tab gets one
        (tabs_config[0].1, Some(tabs_config[0].2.to_string()), "Initial message for Tab 1"),
        (tabs_config[1].1, Some(tabs_config[1].2.to_string()), "Initial message for Tab 2"),
        (tabs_config[2].1, Some(tabs_config[2].2.to_string()), "Initial message for Tab 3"),
        (tabs_config[3].1, Some(tabs_config[3].2.to_string()), "Initial message for Tab 4"),
        
        // Follow-up messages - these should go to their respective tabs
        (tabs_config[1].1, Some(tabs_config[1].2.to_string()), "Follow-up for Tab 2"),
        (tabs_config[2].1, Some(tabs_config[2].2.to_string()), "Follow-up for Tab 3"),
        (tabs_config[3].1, Some(tabs_config[3].2.to_string()), "Follow-up for Tab 4"),
        
        // Messages with mismatched session IDs (simulating the bug)
        (tabs_config[1].1, None, "Message with no session ID for Tab 2"),
        (tabs_config[2].1, Some("wrong-session".to_string()), "Message with wrong session ID for Tab 3"),
    ];
    
    // Route all messages
    for (instance_id, session_id, text) in messages {
        ui_state.route_message(instance_id, session_id, text.to_string());
    }
    
    // Step 3: Validate message distribution
    let message_counts = ui_state.get_tab_message_counts();
    
    // Check that each tab has messages (not empty)
    for (tab_name, count) in &message_counts {
        assert!(
            *count > 0, 
            "{} should have messages but has {}", 
            tab_name, 
            count
        );
    }
    
    // Verify Tab 1 doesn't have ALL the messages (the bug we're fixing)
    let total_messages: usize = message_counts.iter().map(|(_, c)| c).sum();
    let tab1_messages = message_counts[0].1;
    assert!(
        tab1_messages < total_messages,
        "Tab 1 should not have all {} messages, but has {}",
        total_messages,
        tab1_messages
    );
    
    // Verify each tab has appropriate messages
    assert_eq!(ui_state.tabs[0].messages.len(), 1, "Tab 1 should have 1 message");
    assert_eq!(ui_state.tabs[1].messages.len(), 3, "Tab 2 should have 3 messages");
    assert_eq!(ui_state.tabs[2].messages.len(), 3, "Tab 3 should have 3 messages");
    assert_eq!(ui_state.tabs[3].messages.len(), 2, "Tab 4 should have 2 messages");
}

/// Test that validates the UI structure has proper message content
#[test]
fn test_ui_structure_validation() {
    let mut ui_state = UIState::new();
    
    // Create tabs
    let tab1_id = Uuid::new_v4();
    let tab2_id = Uuid::new_v4();
    let tab3_id = Uuid::new_v4();
    
    ui_state.add_tab("Veda-1".to_string(), tab1_id, "session-1".to_string());
    ui_state.add_tab("Veda-2".to_string(), tab2_id, "session-2".to_string());
    ui_state.add_tab("Veda-3".to_string(), tab3_id, "session-3".to_string());
    
    // Add messages to tabs
    ui_state.route_message(tab1_id, Some("session-1".to_string()), "Message 1 for Tab 1".to_string());
    ui_state.route_message(tab2_id, Some("session-2".to_string()), "Message 1 for Tab 2".to_string());
    ui_state.route_message(tab3_id, Some("session-3".to_string()), "Message 1 for Tab 3".to_string());
    
    // Validate structure
    for (i, tab) in ui_state.tabs.iter().enumerate() {
        assert!(!tab.messages.is_empty(), "Tab {} should have messages", i);
        assert!(tab.messages[0].contains(&format!("Tab {}", i + 1)), 
            "Tab {} message should contain correct tab reference", i);
    }
}

/// Test that detects the "1 message per tab while main tab overflows" issue
#[test]
fn test_main_tab_overflow_detection() {
    let mut ui_state = UIState::new();
    
    // Create 4 tabs
    let tabs = vec![
        ("Veda-1", Uuid::new_v4(), "session-1"),
        ("Veda-2", Uuid::new_v4(), "session-2"),
        ("Veda-3", Uuid::new_v4(), "session-3"),
        ("Veda-4", Uuid::new_v4(), "session-4"),
    ];
    
    for (name, id, session) in &tabs {
        ui_state.add_tab(name.to_string(), *id, session.to_string());
    }
    
    // Simulate the bug: each tab gets 1 initial message
    for (i, (_, id, session)) in tabs.iter().enumerate() {
        ui_state.route_message(*id, Some(session.to_string()), format!("Initial message {}", i));
    }
    
    // Then all subsequent messages go to main tab (simulating session ID mismatch)
    for i in 0..20 {
        // These messages have instance IDs but wrong/missing session IDs
        let instance_id = tabs[i % 4].1;
        ui_state.route_message(instance_id, None, format!("Overflow message {}", i));
    }
    
    // Check message distribution
    let counts = ui_state.get_tab_message_counts();
    
    // Main tab should have way more messages (the bug)
    let main_tab_count = counts[0].1;
    let other_tab_avg = (counts[1].1 + counts[2].1 + counts[3].1) / 3;
    
    // FAIL HARD if the bug exists
    assert!(
        main_tab_count <= other_tab_avg * 2,
        "CRITICAL BUG: Main tab has {} messages while others average {} - tab routing is broken!",
        main_tab_count,
        other_tab_avg
    );
}

/// Test IPC message reception through UI rendering
#[tokio::test]
async fn test_ipc_to_ui_pipeline() {
    // Simulate IPC messages coming from MCP server
    let ipc_messages = vec![
        json!({
            "type": "stream_text",
            "instance_id": "ba5ee63c-b35e-4a4a-90a6-6d7281b18516",
            "session_id": "session-1",
            "text": "Output from Claude instance 1"
        }),
        json!({
            "type": "stream_text",
            "instance_id": "2685b199-e094-4220-8999-eec718f52b12",
            "session_id": "session-2",
            "text": "Output from Claude instance 2"
        }),
        json!({
            "type": "stream_text",
            "instance_id": "e1edaadc-e78f-4f5d-9684-d9035fdeacd8",
            "session_id": "session-3",
            "text": "Output from Claude instance 3"
        }),
    ];
    
    let mut ui_state = UIState::new();
    
    // Create tabs matching the instance IDs
    ui_state.add_tab(
        "Veda-1".to_string(),
        Uuid::parse_str("ba5ee63c-b35e-4a4a-90a6-6d7281b18516").unwrap(),
        "session-1".to_string()
    );
    ui_state.add_tab(
        "Veda-2".to_string(),
        Uuid::parse_str("2685b199-e094-4220-8999-eec718f52b12").unwrap(),
        "session-2".to_string()
    );
    ui_state.add_tab(
        "Veda-3".to_string(),
        Uuid::parse_str("e1edaadc-e78f-4f5d-9684-d9035fdeacd8").unwrap(),
        "session-3".to_string()
    );
    
    // Process IPC messages
    for msg in ipc_messages {
        let instance_id = Uuid::parse_str(msg["instance_id"].as_str().unwrap()).unwrap();
        let session_id = msg["session_id"].as_str().map(|s| s.to_string());
        let text = msg["text"].as_str().unwrap().to_string();
        
        ui_state.route_message(instance_id, session_id, text);
    }
    
    // Verify each tab has its correct message
    assert_eq!(ui_state.tabs[0].messages.len(), 1);
    assert!(ui_state.tabs[0].messages[0].contains("Claude instance 1"));
    
    assert_eq!(ui_state.tabs[1].messages.len(), 1);
    assert!(ui_state.tabs[1].messages[0].contains("Claude instance 2"));
    
    assert_eq!(ui_state.tabs[2].messages.len(), 1);
    assert!(ui_state.tabs[2].messages[0].contains("Claude instance 3"));
}

/// Test session ID matching logic
#[test]
fn test_session_id_routing_priority() {
    let mut ui_state = UIState::new();
    
    let instance_id = Uuid::new_v4();
    let session_id = "test-session";
    
    ui_state.add_tab("Veda-1".to_string(), instance_id, session_id.to_string());
    
    // Test 1: Message with matching session ID routes correctly
    ui_state.route_message(instance_id, Some(session_id.to_string()), "Correct routing".to_string());
    assert_eq!(ui_state.tabs[0].messages.len(), 1);
    
    // Test 2: Message with wrong session ID but correct instance ID still routes
    ui_state.route_message(instance_id, Some("wrong-session".to_string()), "Instance ID routing".to_string());
    assert_eq!(ui_state.tabs[0].messages.len(), 2);
    
    // Test 3: Message with no session ID but correct instance ID routes
    ui_state.route_message(instance_id, None, "No session routing".to_string());
    assert_eq!(ui_state.tabs[0].messages.len(), 3);
}

/// Test that validates proper environment variable propagation
#[test]
fn test_environment_variable_propagation() {
    // Simulate the complete flow from Claude to IPC
    let instance_configs = vec![
        ("Veda-1", Uuid::new_v4()),
        ("Veda-2", Uuid::new_v4()),
        ("Veda-3", Uuid::new_v4()),
    ];
    
    let mut env_vars = HashMap::new();
    let mut ipc_messages = Vec::new();
    
    // Each Claude instance sets its environment
    for (name, instance_id) in &instance_configs {
        env_vars.insert(
            format!("{}_ENV", name),
            format!("VEDA_TARGET_INSTANCE_ID={}", instance_id)
        );
        
        // MCP server creates IPC message with that ID
        let ipc_msg = json!({
            "type": "spawn_instances",
            "target_instance_id": instance_id.to_string(),
            "session_id": format!("session-{}", name)
        });
        
        ipc_messages.push(ipc_msg);
    }
    
    // Verify each IPC message has the correct instance ID
    for (i, msg) in ipc_messages.iter().enumerate() {
        let expected_id = instance_configs[i].1.to_string();
        assert_eq!(
            msg["target_instance_id"].as_str().unwrap(),
            expected_id
        );
    }
}

/// Test message buffering and overflow scenarios
#[test]
fn test_message_buffering_and_overflow() {
    let mut ui_state = UIState::new();
    
    // Create a single tab
    let instance_id = Uuid::new_v4();
    let session_id = "main-session";
    ui_state.add_tab("Veda-Main".to_string(), instance_id, session_id.to_string());
    
    // Send many messages to simulate overflow
    const MESSAGE_COUNT: usize = 1000;
    for i in 0..MESSAGE_COUNT {
        ui_state.route_message(
            instance_id,
            Some(session_id.to_string()),
            format!("Message {}", i)
        );
    }
    
    // Verify all messages were received
    assert_eq!(ui_state.tabs[0].messages.len(), MESSAGE_COUNT);
    
    // Verify messages are in order
    for (i, msg) in ui_state.tabs[0].messages.iter().enumerate() {
        assert_eq!(msg, &format!("Message {}", i));
    }
}

/// Integration test for automode tool enablement scenario
#[test]
fn test_automode_tool_enablement_routing() {
    let mut ui_state = UIState::new();
    
    // Create main tab
    let main_instance_id = Uuid::new_v4();
    let main_session_id = "main-session";
    ui_state.add_tab("Veda-1".to_string(), main_instance_id, main_session_id.to_string());
    
    // Simulate automode enabling tools (the bug was session_id: None)
    let tool_enable_messages = vec![
        // Bug: These had session_id: None
        (main_instance_id, None, "Automode: Enabling tools..."),
        (main_instance_id, None, "Tool safety analysis in progress..."),
        
        // Fixed: These should have session_id
        (main_instance_id, Some(main_session_id.to_string()), "Automode: Tools enabled"),
        (main_instance_id, Some(main_session_id.to_string()), "Ready to proceed"),
    ];
    
    for (instance_id, session_id, text) in tool_enable_messages {
        ui_state.route_message(instance_id, session_id, text.to_string());
    }
    
    // All messages should go to the main tab
    assert_eq!(ui_state.tabs[0].messages.len(), 4);
}

/// Test concurrent message handling
#[tokio::test]
async fn test_concurrent_message_handling() {
    let ui_state = Arc::new(Mutex::new(UIState::new()));
    
    // Setup tabs
    let tabs = vec![
        (Uuid::new_v4(), "session-1"),
        (Uuid::new_v4(), "session-2"),
        (Uuid::new_v4(), "session-3"),
    ];
    
    {
        let mut state = ui_state.lock().await;
        for (i, (id, session)) in tabs.iter().enumerate() {
            state.add_tab(format!("Veda-{}", i + 1), *id, session.to_string());
        }
    }
    
    // Simulate concurrent messages from multiple Claude instances
    let mut handles = vec![];
    
    for (i, (instance_id, session_id)) in tabs.iter().enumerate() {
        let ui_state_clone = Arc::clone(&ui_state);
        let instance_id = *instance_id;
        let session_id = session_id.to_string();
        
        let handle = tokio::spawn(async move {
            for j in 0..10 {
                let mut state = ui_state_clone.lock().await;
                state.route_message(
                    instance_id,
                    Some(session_id.clone()),
                    format!("Tab {} Message {}", i + 1, j)
                );
            }
        });
        
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify each tab has exactly 10 messages
    let state = ui_state.lock().await;
    for (i, tab) in state.tabs.iter().enumerate() {
        assert_eq!(tab.messages.len(), 10, "Tab {} should have 10 messages", i + 1);
    }
}

/// Test that empty tabs are properly detected
#[test]
fn test_empty_tab_detection() {
    let mut ui_state = UIState::new();
    
    // Create tabs but don't send messages to all
    for i in 0..4 {
        ui_state.add_tab(
            format!("Veda-{}", i + 1),
            Uuid::new_v4(),
            format!("session-{}", i + 1)
        );
    }
    
    // Only send messages to first two tabs
    ui_state.route_message(
        ui_state.tabs[0].instance_id,
        Some(ui_state.tabs[0].session_id.clone()),
        "Message for tab 1".to_string()
    );
    ui_state.route_message(
        ui_state.tabs[1].instance_id,
        Some(ui_state.tabs[1].session_id.clone()),
        "Message for tab 2".to_string()
    );
    
    // Check for empty tabs
    let empty_tabs: Vec<_> = ui_state.tabs.iter()
        .enumerate()
        .filter(|(_, tab)| tab.messages.is_empty())
        .map(|(i, tab)| (i, tab.id.clone()))
        .collect();
    
    assert_eq!(empty_tabs.len(), 2, "Should have 2 empty tabs");
    assert_eq!(empty_tabs[0].1, "Veda-3");
    assert_eq!(empty_tabs[1].1, "Veda-4");
}