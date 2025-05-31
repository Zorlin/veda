use serde_json::{json, Value};
use uuid::Uuid;
use std::collections::{HashMap, HashSet};
use chrono::Utc;

/// Message structure that matches what Veda uses
#[derive(Debug, Clone)]
struct VedaMessage {
    instance_id: Uuid,
    session_id: Option<String>,
    text: String,
    timestamp: i64,
    message_type: String,
}

/// Tab state that tracks messages and metadata
#[derive(Debug)]
struct TabState {
    instance_id: Uuid,
    session_id: String,
    name: String,
    messages: Vec<VedaMessage>,
    created_at: i64,
    last_message_at: Option<i64>,
}

impl TabState {
    fn new(name: String, instance_id: Uuid, session_id: String) -> Self {
        Self {
            instance_id,
            session_id,
            name,
            messages: Vec::new(),
            created_at: Utc::now().timestamp_millis(),
            last_message_at: None,
        }
    }
    
    fn add_message(&mut self, msg: VedaMessage) {
        self.last_message_at = Some(msg.timestamp);
        self.messages.push(msg);
    }
    
    fn has_only_initial_message(&self) -> bool {
        self.messages.len() == 1
    }
    
    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    
    fn message_count(&self) -> usize {
        self.messages.len()
    }
}

/// Test session ID mismatches that cause routing failures
#[test]
fn test_session_id_mismatch_detection() {
    let mut tabs = HashMap::new();
    
    // Create tabs with specific session IDs
    let tab1 = TabState::new(
        "Veda-1".to_string(),
        Uuid::new_v4(),
        "d4abc3c6-9dcd-4f06-97b1-5f6aa00b7489".to_string()
    );
    let tab2 = TabState::new(
        "Veda-2".to_string(),
        Uuid::new_v4(),
        "26afcb8f-06c5-463b-b22f-fbb0d3dfbc02".to_string()
    );
    let tab3 = TabState::new(
        "Veda-3".to_string(),
        Uuid::new_v4(),
        "e5f7c890-1234-5678-9abc-def012345678".to_string()
    );
    
    tabs.insert(tab1.instance_id, tab1);
    tabs.insert(tab2.instance_id, tab2);
    tabs.insert(tab3.instance_id, tab3);
    
    // Messages with mismatched session IDs (simulating the bug)
    let messages = vec![
        VedaMessage {
            instance_id: tabs.values().nth(0).unwrap().instance_id,
            session_id: Some("wrong-session-1".to_string()), // Wrong session ID!
            text: "This message has wrong session ID".to_string(),
            timestamp: Utc::now().timestamp_millis(),
            message_type: "StreamText".to_string(),
        },
        VedaMessage {
            instance_id: tabs.values().nth(1).unwrap().instance_id,
            session_id: None, // No session ID!
            text: "This message has no session ID".to_string(),
            timestamp: Utc::now().timestamp_millis(),
            message_type: "StreamText".to_string(),
        },
        VedaMessage {
            instance_id: tabs.values().nth(2).unwrap().instance_id,
            session_id: Some(tabs.values().nth(2).unwrap().session_id.clone()), // Correct
            text: "This message has correct session ID".to_string(),
            timestamp: Utc::now().timestamp_millis(),
            message_type: "StreamText".to_string(),
        },
    ];
    
    // Track routing failures
    let mut routing_failures = Vec::new();
    
    for msg in messages {
        let mut routed = false;
        
        // Try to route by session ID first
        if let Some(ref sid) = msg.session_id {
            for tab in tabs.values_mut() {
                if tab.session_id == *sid {
                    tab.add_message(msg.clone());
                    routed = true;
                    break;
                }
            }
        }
        
        // If not routed by session, try instance ID
        if !routed {
            if let Some(tab) = tabs.get_mut(&msg.instance_id) {
                tab.add_message(msg.clone());
                routed = true;
            }
        }
        
        if !routed {
            routing_failures.push(msg);
        }
    }
    
    // Verify routing behavior
    assert_eq!(routing_failures.len(), 0, "All messages should be routed");
    
    // Check which tabs received messages
    let tabs_with_messages: Vec<_> = tabs.values()
        .filter(|t| !t.is_empty())
        .map(|t| t.name.clone())
        .collect();
    
    assert_eq!(tabs_with_messages.len(), 3, "All tabs should have messages");
}

/// Test the "1 message per tab while main overflows" bug
#[test]
fn test_one_message_per_tab_bug() {
    let mut tabs = Vec::new();
    
    // Create 4 tabs
    for i in 0..4 {
        tabs.push(TabState::new(
            format!("Veda-{}", i + 1),
            Uuid::new_v4(),
            format!("session-{}", i + 1)
        ));
    }
    
    // Each tab gets one initial message with correct session ID
    for (i, tab) in tabs.iter_mut().enumerate() {
        tab.add_message(VedaMessage {
            instance_id: tab.instance_id,
            session_id: Some(tab.session_id.clone()),
            text: format!("Initial message for tab {}", i + 1),
            timestamp: Utc::now().timestamp_millis(),
            message_type: "StreamText".to_string(),
        });
    }
    
    // Simulate the bug: subsequent messages have None or wrong session IDs
    // so they all go to the main tab
    let main_tab_id = tabs[0].instance_id;
    for i in 0..50 {
        let target_tab = i % 4;
        let msg = VedaMessage {
            instance_id: tabs[target_tab].instance_id,
            session_id: None, // BUG: No session ID causes routing to main
            text: format!("Message {} intended for tab {}", i, target_tab + 1),
            timestamp: Utc::now().timestamp_millis(),
            message_type: "StreamText".to_string(),
        };
        
        // Due to the bug, this goes to main tab instead of target tab
        tabs[0].add_message(msg);
    }
    
    // Check the bug symptoms
    let message_distribution: Vec<_> = tabs.iter()
        .map(|t| (t.name.clone(), t.message_count()))
        .collect();
    
    println!("Message distribution: {:?}", message_distribution);
    
    // Bug symptoms:
    // 1. Each non-main tab has only 1 message (THIS IS BAD)
    assert!(!tabs[1].has_only_initial_message(), "Tab 2 has only 1 message - routing bug detected!");
    assert!(!tabs[2].has_only_initial_message(), "Tab 3 has only 1 message - routing bug detected!");
    assert!(!tabs[3].has_only_initial_message(), "Tab 4 has only 1 message - routing bug detected!");
    
    // 2. Main tab should NOT have all the messages
    assert!(tabs[0].message_count() <= 40, "Main tab has {} messages - overflow bug detected!", tabs[0].message_count());
}

/// Test message structure validation
#[test]
fn test_message_structure_validation() {
    // Valid message structure
    let valid_msg = json!({
        "type": "StreamText",
        "instance_id": "ba5ee63c-b35e-4a4a-90a6-6d7281b18516",
        "session_id": "test-session",
        "text": "Valid message"
    });
    
    // Invalid structures
    let invalid_messages = vec![
        // Missing instance_id
        json!({
            "type": "StreamText",
            "session_id": "test-session",
            "text": "Missing instance_id"
        }),
        // Invalid instance_id format
        json!({
            "type": "StreamText",
            "instance_id": "not-a-uuid",
            "session_id": "test-session",
            "text": "Invalid instance_id"
        }),
        // Missing text
        json!({
            "type": "StreamText",
            "instance_id": "ba5ee63c-b35e-4a4a-90a6-6d7281b18516",
            "session_id": "test-session"
        }),
    ];
    
    // Validate structure
    fn validate_message(msg: &Value) -> Result<(), String> {
        // Check required fields
        if !msg.get("instance_id").is_some() {
            return Err("Missing instance_id".to_string());
        }
        
        // Validate instance_id is valid UUID
        if let Some(id_str) = msg["instance_id"].as_str() {
            if Uuid::parse_str(id_str).is_err() {
                return Err("Invalid UUID format for instance_id".to_string());
            }
        } else {
            return Err("instance_id must be a string".to_string());
        }
        
        // Check text field
        if !msg.get("text").is_some() {
            return Err("Missing text field".to_string());
        }
        
        Ok(())
    }
    
    // Valid message should pass
    assert!(validate_message(&valid_msg).is_ok());
    
    // Invalid messages should fail
    for (i, invalid_msg) in invalid_messages.iter().enumerate() {
        assert!(
            validate_message(invalid_msg).is_err(),
            "Invalid message {} should fail validation",
            i
        );
    }
}

/// Test automode session ID issues
#[test]
fn test_automode_session_id_problems() {
    let main_instance_id = Uuid::new_v4();
    let main_session_id = "main-session";
    
    // Messages from automode with session ID issues
    let automode_messages = vec![
        // Bug: automode sends with session_id: None
        json!({
            "type": "StreamText",
            "instance_id": main_instance_id.to_string(),
            "session_id": null,
            "text": "Automode: Enabling tools..."
        }),
        json!({
            "type": "StreamText",
            "instance_id": main_instance_id.to_string(),
            "session_id": null,
            "text": "Tool safety analysis..."
        }),
        // Fixed: should have session_id
        json!({
            "type": "StreamText",
            "instance_id": main_instance_id.to_string(),
            "session_id": main_session_id,
            "text": "Tools enabled successfully"
        }),
    ];
    
    // Count messages with and without session IDs
    let mut with_session = 0;
    let mut without_session = 0;
    
    for msg in &automode_messages {
        if msg["session_id"].is_null() {
            without_session += 1;
        } else {
            with_session += 1;
        }
    }
    
    assert_eq!(without_session, 2, "Found {} messages without session_id", without_session);
    assert_eq!(with_session, 1, "Found {} messages with session_id", with_session);
}

/// Test session resumption and ID consistency
#[test]
fn test_session_resumption_consistency() {
    // When resuming a session, instance IDs change but session IDs should remain
    let original_session_id = "persistent-session-123";
    let original_instance_id = Uuid::new_v4();
    let resumed_instance_id = Uuid::new_v4();
    
    // Original tab
    let mut original_tab = TabState::new(
        "Veda-1".to_string(),
        original_instance_id,
        original_session_id.to_string()
    );
    
    // Add messages to original
    original_tab.add_message(VedaMessage {
        instance_id: original_instance_id,
        session_id: Some(original_session_id.to_string()),
        text: "Message before resume".to_string(),
        timestamp: Utc::now().timestamp_millis(),
        message_type: "StreamText".to_string(),
    });
    
    // After resume - new instance ID but same session ID
    let mut resumed_tab = TabState::new(
        "Veda-1".to_string(),
        resumed_instance_id,
        original_session_id.to_string()
    );
    
    // Messages after resume should use new instance ID
    resumed_tab.add_message(VedaMessage {
        instance_id: resumed_instance_id,
        session_id: Some(original_session_id.to_string()),
        text: "Message after resume".to_string(),
        timestamp: Utc::now().timestamp_millis(),
        message_type: "StreamText".to_string(),
    });
    
    // Verify session consistency
    assert_eq!(original_tab.session_id, resumed_tab.session_id);
    assert_ne!(original_tab.instance_id, resumed_tab.instance_id);
}

/// Test complex routing scenarios
#[test]
fn test_complex_routing_scenarios() {
    #[derive(Debug)]
    struct RoutingTest {
        name: String,
        instance_id: Uuid,
        session_id: Option<String>,
        expected_tab_index: usize,
        reason: String,
    }
    
    let tabs = vec![
        TabState::new("Veda-1".to_string(), Uuid::new_v4(), "session-1".to_string()),
        TabState::new("Veda-2".to_string(), Uuid::new_v4(), "session-2".to_string()),
        TabState::new("Veda-3".to_string(), Uuid::new_v4(), "session-3".to_string()),
    ];
    
    let tests = vec![
        RoutingTest {
            name: "Correct session and instance".to_string(),
            instance_id: tabs[0].instance_id,
            session_id: Some(tabs[0].session_id.clone()),
            expected_tab_index: 0,
            reason: "Should route to tab 1".to_string(),
        },
        RoutingTest {
            name: "Wrong session but correct instance".to_string(),
            instance_id: tabs[1].instance_id,
            session_id: Some("wrong-session".to_string()),
            expected_tab_index: 1,
            reason: "Should fallback to instance ID routing".to_string(),
        },
        RoutingTest {
            name: "No session but correct instance".to_string(),
            instance_id: tabs[2].instance_id,
            session_id: None,
            expected_tab_index: 2,
            reason: "Should use instance ID when no session".to_string(),
        },
        RoutingTest {
            name: "Session from different tab".to_string(),
            instance_id: tabs[0].instance_id,
            session_id: Some(tabs[1].session_id.clone()),
            expected_tab_index: 1,
            reason: "Session ID takes priority".to_string(),
        },
    ];
    
    // Run routing tests
    for test in tests {
        println!("Running test: {} - {}", test.name, test.reason);
        
        // Simulate routing logic
        let mut routed_to_index = None;
        
        // Priority 1: Session ID
        if let Some(ref sid) = test.session_id {
            for (i, tab) in tabs.iter().enumerate() {
                if tab.session_id == *sid {
                    routed_to_index = Some(i);
                    break;
                }
            }
        }
        
        // Priority 2: Instance ID
        if routed_to_index.is_none() {
            for (i, tab) in tabs.iter().enumerate() {
                if tab.instance_id == test.instance_id {
                    routed_to_index = Some(i);
                    break;
                }
            }
        }
        
        assert_eq!(
            routed_to_index.unwrap(),
            test.expected_tab_index,
            "Test '{}' failed: {}",
            test.name,
            test.reason
        );
    }
}

/// Test that validates tabs don't become empty due to routing issues
#[test]
fn test_tabs_not_empty_validation() {
    let mut tabs = HashMap::new();
    
    // Create 4 tabs
    for i in 0..4 {
        let tab = TabState::new(
            format!("Veda-{}", i + 1),
            Uuid::new_v4(),
            format!("session-{}", i + 1)
        );
        tabs.insert(tab.instance_id, tab);
    }
    
    // Send at least one message to each tab
    for tab in tabs.values_mut() {
        tab.add_message(VedaMessage {
            instance_id: tab.instance_id,
            session_id: Some(tab.session_id.clone()),
            text: format!("Message for {}", tab.name),
            timestamp: Utc::now().timestamp_millis(),
            message_type: "StreamText".to_string(),
        });
    }
    
    // Validate no tabs are empty
    let empty_tabs: Vec<_> = tabs.values()
        .filter(|t| t.is_empty())
        .map(|t| t.name.clone())
        .collect();
    
    assert!(
        empty_tabs.is_empty(),
        "Found empty tabs: {:?}",
        empty_tabs
    );
    
    // Validate tabs have reasonable message counts
    for tab in tabs.values() {
        assert!(
            tab.message_count() > 0,
            "Tab {} should have messages",
            tab.name
        );
    }
}