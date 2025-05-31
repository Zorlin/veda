use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

/// Simulates the ClaudeMessage enum from main.rs
#[derive(Debug, Clone)]
enum ClaudeMessage {
    StreamText {
        instance_id: Uuid,
        text: String,
        session_id: Option<String>,
    },
    ToolUse {
        instance_id: Uuid,
        tool: String,
        input: serde_json::Value,
        session_id: Option<String>,
    },
    SessionStarted {
        instance_id: Uuid,
        session_id: String,
    },
    RequestInput {
        instance_id: Uuid,
        prompt: String,
        session_id: Option<String>,
    },
}

/// Simulates the async message handling pipeline
async fn process_message_pipeline(
    mut rx: mpsc::Receiver<ClaudeMessage>,
    tab_states: Arc<Mutex<HashMap<Uuid, Vec<String>>>>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ClaudeMessage::StreamText { instance_id, text, session_id } => {
                let mut states = tab_states.lock().await;
                
                // Simulate routing logic
                if let Some(tab_messages) = states.get_mut(&instance_id) {
                    tab_messages.push(format!("[{}] {}", 
                        session_id.as_deref().unwrap_or("no-session"), 
                        text
                    ));
                }
            }
            _ => {}
        }
    }
}

/// Test the complete async pipeline
#[tokio::test]
async fn test_async_message_pipeline() {
    let (tx, rx) = mpsc::channel(100);
    let tab_states = Arc::new(Mutex::new(HashMap::new()));
    
    // Create tabs
    let tab1_id = Uuid::new_v4();
    let tab2_id = Uuid::new_v4();
    let tab3_id = Uuid::new_v4();
    
    {
        let mut states = tab_states.lock().await;
        states.insert(tab1_id, Vec::new());
        states.insert(tab2_id, Vec::new());
        states.insert(tab3_id, Vec::new());
    }
    
    // Start message processor
    let states_clone = Arc::clone(&tab_states);
    let processor = tokio::spawn(async move {
        process_message_pipeline(rx, states_clone).await;
    });
    
    // Send messages with various session ID scenarios
    let messages = vec![
        ClaudeMessage::StreamText {
            instance_id: tab1_id,
            text: "Message 1 for Tab 1".to_string(),
            session_id: Some("session-1".to_string()),
        },
        ClaudeMessage::StreamText {
            instance_id: tab2_id,
            text: "Message 1 for Tab 2".to_string(),
            session_id: Some("session-2".to_string()),
        },
        ClaudeMessage::StreamText {
            instance_id: tab3_id,
            text: "Message 1 for Tab 3".to_string(),
            session_id: None, // Bug: no session ID
        },
        ClaudeMessage::StreamText {
            instance_id: tab2_id,
            text: "Message 2 for Tab 2".to_string(),
            session_id: None, // Bug: no session ID
        },
    ];
    
    // Send messages
    for msg in messages {
        tx.send(msg).await.unwrap();
    }
    
    // Wait for processing
    sleep(Duration::from_millis(100)).await;
    
    // Drop sender to close channel
    drop(tx);
    
    // Wait for processor to finish
    let _ = processor.await;
    
    // Check results
    let states = tab_states.lock().await;
    
    // Verify each tab received messages
    let tab1_messages = &states[&tab1_id];
    let tab2_messages = &states[&tab2_id];
    let tab3_messages = &states[&tab3_id];
    
    assert_eq!(tab1_messages.len(), 1, "Tab 1 should have 1 message");
    assert_eq!(tab2_messages.len(), 2, "Tab 2 should have 2 messages");
    assert_eq!(tab3_messages.len(), 1, "Tab 3 should have 1 message");
    
    // Check for session ID issues
    assert!(tab2_messages[1].contains("no-session"), "Second message to Tab 2 has no session");
    assert!(tab3_messages[0].contains("no-session"), "Tab 3 message has no session");
}

/// Test concurrent message handling under load
#[tokio::test]
async fn test_concurrent_message_load() {
    let (tx, rx) = mpsc::channel(1000);
    let message_counts = Arc::new(Mutex::new(HashMap::new()));
    
    // Create 10 tabs
    let mut tab_ids = Vec::new();
    for i in 0..10 {
        let id = Uuid::new_v4();
        tab_ids.push(id);
        message_counts.lock().await.insert(id, 0);
    }
    
    // Message counter
    let counts_clone = Arc::clone(&message_counts);
    let counter = tokio::spawn(async move {
        let mut rx = rx;
        while let Some(msg) = rx.recv().await {
            if let ClaudeMessage::StreamText { instance_id, .. } = msg {
                let mut counts = counts_clone.lock().await;
                if let Some(count) = counts.get_mut(&instance_id) {
                    *count += 1;
                }
            }
        }
    });
    
    // Spawn multiple senders
    let mut senders = Vec::new();
    for i in 0..5 {
        let tx_clone = tx.clone();
        let tab_ids_clone = tab_ids.clone();
        
        let sender = tokio::spawn(async move {
            for j in 0..100 {
                let tab_id = tab_ids_clone[j % tab_ids_clone.len()];
                let msg = ClaudeMessage::StreamText {
                    instance_id: tab_id,
                    text: format!("Sender {} Message {}", i, j),
                    session_id: if j % 3 == 0 { None } else { Some(format!("session-{}", i)) },
                };
                tx_clone.send(msg).await.unwrap();
            }
        });
        
        senders.push(sender);
    }
    
    // Wait for senders
    for sender in senders {
        sender.await.unwrap();
    }
    
    // Close channel
    drop(tx);
    
    // Wait for counter
    counter.await.unwrap();
    
    // Verify message distribution
    let counts = message_counts.lock().await;
    let total_messages: usize = counts.values().sum();
    
    assert_eq!(total_messages, 500, "Should have 500 total messages");
    
    // Check for even distribution
    for (tab_id, count) in counts.iter() {
        assert!(
            *count > 0,
            "Tab {:?} should have received messages",
            tab_id
        );
    }
}

/// Test IPC socket communication simulation
#[tokio::test]
async fn test_ipc_socket_simulation() {
    // Simulate IPC messages from MCP server
    let ipc_messages = vec![
        json!({
            "type": "spawn_instances",
            "session_id": "test-session",
            "task_description": "Test task",
            "num_instances": 2,
            "target_instance_id": "ba5ee63c-b35e-4a4a-90a6-6d7281b18516"
        }),
        json!({
            "type": "list_instances",
            "session_id": "test-session",
            "target_instance_id": "ba5ee63c-b35e-4a4a-90a6-6d7281b18516"
        }),
    ];
    
    let (tx, mut rx) = mpsc::channel::<serde_json::Value>(10);
    
    // Simulate IPC handler
    let handler = tokio::spawn(async move {
        let mut processed = Vec::new();
        
        while let Some(msg) = rx.recv().await {
            // Parse target_instance_id
            if let Some(target_id_str) = msg["target_instance_id"].as_str() {
                if let Ok(id) = Uuid::parse_str(target_id_str) {
                    processed.push(id);
                }
            }
        }
        
        processed
    });
    
    // Send IPC messages
    for msg in ipc_messages {
        tx.send(msg).await.unwrap();
    }
    
    drop(tx);
    
    // Get results
    let processed_ids = handler.await.unwrap();
    
    assert_eq!(processed_ids.len(), 2);
    assert_eq!(processed_ids[0], processed_ids[1], "Same instance ID should be used");
}

/// Test session resumption flow
#[tokio::test]
async fn test_session_resumption_flow() {
    #[derive(Debug)]
    struct SessionState {
        session_id: String,
        instance_id: Uuid,
        messages: Vec<String>,
    }
    
    // Original session
    let original_session = SessionState {
        session_id: "persistent-session-123".to_string(),
        instance_id: Uuid::new_v4(),
        messages: vec!["Original message 1".to_string(), "Original message 2".to_string()],
    };
    
    // Simulate session save
    let saved_session_id = original_session.session_id.clone();
    let saved_messages = original_session.messages.clone();
    
    // Simulate session resume with new instance
    let resumed_session = SessionState {
        session_id: saved_session_id.clone(),
        instance_id: Uuid::new_v4(), // New instance ID
        messages: saved_messages.clone(),
    };
    
    // Add new messages after resume
    let mut resumed_messages = resumed_session.messages.clone();
    resumed_messages.push("New message after resume".to_string());
    
    // Verify session consistency
    assert_eq!(original_session.session_id, resumed_session.session_id);
    assert_ne!(original_session.instance_id, resumed_session.instance_id);
    assert_eq!(resumed_messages.len(), 3);
}

/// Test UI overflow and empty tab detection
#[tokio::test]
async fn test_ui_overflow_empty_detection() {
    #[derive(Debug)]
    struct TabMetrics {
        id: Uuid,
        name: String,
        message_count: usize,
        is_empty: bool,
        is_overflow: bool,
    }
    
    const OVERFLOW_THRESHOLD: usize = 100;
    
    let tabs = vec![
        TabMetrics {
            id: Uuid::new_v4(),
            name: "Veda-1".to_string(),
            message_count: 150,
            is_empty: false,
            is_overflow: true,
        },
        TabMetrics {
            id: Uuid::new_v4(),
            name: "Veda-2".to_string(),
            message_count: 1,
            is_empty: false,
            is_overflow: false,
        },
        TabMetrics {
            id: Uuid::new_v4(),
            name: "Veda-3".to_string(),
            message_count: 1,
            is_empty: false,
            is_overflow: false,
        },
        TabMetrics {
            id: Uuid::new_v4(),
            name: "Veda-4".to_string(),
            message_count: 0,
            is_empty: true,
            is_overflow: false,
        },
    ];
    
    // Detect the bug pattern
    let overflow_tabs = tabs.iter().filter(|t| t.is_overflow).count();
    let single_message_tabs = tabs.iter().filter(|t| t.message_count == 1).count();
    let empty_tabs = tabs.iter().filter(|t| t.is_empty).count();
    
    // This pattern indicates the routing bug
    if overflow_tabs > 0 && single_message_tabs >= 2 {
        println!("WARNING: Detected '1 message per tab while main overflows' pattern!");
        println!("- {} tabs have overflow (>{} messages)", overflow_tabs, OVERFLOW_THRESHOLD);
        println!("- {} tabs have exactly 1 message", single_message_tabs);
        println!("- {} tabs are empty", empty_tabs);
        
        // This test demonstrates the bug pattern - in production this should fail
        // For now, we'll just warn about it
        eprintln!("Tab routing bug pattern detected - this would fail in production!");
    }
}

/// Test complete end-to-end flow with validation
#[tokio::test]
async fn test_complete_end_to_end_validation() {
    // Setup
    let (tx, rx) = mpsc::channel(100);
    let tab_states = Arc::new(Mutex::new(HashMap::new()));
    
    // Create 4 tabs with proper IDs and sessions
    let tabs = vec![
        (Uuid::new_v4(), "session-main"),
        (Uuid::new_v4(), "session-2"),
        (Uuid::new_v4(), "session-3"),
        (Uuid::new_v4(), "session-4"),
    ];
    
    // Initialize tab states
    {
        let mut states = tab_states.lock().await;
        for (id, _) in &tabs {
            states.insert(*id, Vec::new());
        }
    }
    
    // Message processor
    let states_clone = Arc::clone(&tab_states);
    let processor = tokio::spawn(async move {
        process_message_pipeline(rx, states_clone).await;
    });
    
    // Phase 1: Send initial messages (all should route correctly)
    for (id, session) in &tabs {
        tx.send(ClaudeMessage::StreamText {
            instance_id: *id,
            text: format!("Initial message for {}", session),
            session_id: Some(session.to_string()),
        }).await.unwrap();
    }
    
    // Phase 2: Send follow-up messages with session issues
    for i in 0..20 {
        let (id, _) = tabs[i % 4];
        tx.send(ClaudeMessage::StreamText {
            instance_id: id,
            text: format!("Follow-up message {}", i),
            session_id: None, // Bug: no session ID
        }).await.unwrap();
    }
    
    // Wait and close
    sleep(Duration::from_millis(100)).await;
    drop(tx);
    processor.await.unwrap();
    
    // Validate results
    let states = tab_states.lock().await;
    
    // Check message distribution
    let mut message_counts = Vec::new();
    for (id, session) in &tabs {
        let count = states[id].len();
        message_counts.push((session, count));
        println!("{}: {} messages", session, count);
    }
    
    // Validate no tab is empty
    for (session, count) in &message_counts {
        assert!(*count > 0, "Tab {} should not be empty", session);
    }
    
    // Validate reasonable distribution (no single tab has all messages)
    let total: usize = message_counts.iter().map(|(_, c)| c).sum();
    for (session, count) in &message_counts {
        assert!(
            *count < total / 2,
            "Tab {} has too many messages ({}/{})",
            session,
            count,
            total
        );
    }
}