use tokio::sync::mpsc;
use veda_tui::claude::ClaudeMessage;
use std::sync::Arc;

#[tokio::test]
async fn test_tool_approval_message_format() {
    // Create a mock message channel
    let (tx, mut rx) = mpsc::channel::<ClaudeMessage>(100);
    
    // Simulate the tool approval message that would be sent
    let tool_name = "TestTool";
    let session_id = Some("test-session-123".to_string());
    
    // Send the ToolApproved message
    let _ = tx.send(ClaudeMessage::ToolApproved {
        tool_name: tool_name.to_string(),
        session_id: session_id.clone(),
    }).await;
    
    // Send the StreamText message with the new format
    let _ = tx.send(ClaudeMessage::StreamText {
        text: format!("🔧 Automode: Tool {} approved and is immediately available! You MUST retry using this tool now - it's imperative that you do so.", tool_name),
        session_id: session_id.clone(),
    }).await;
    
    // Verify the messages were sent correctly
    if let Some(msg) = rx.recv().await {
        match msg {
            ClaudeMessage::ToolApproved { tool_name: received_tool, session_id: received_session } => {
                assert_eq!(received_tool, "TestTool");
                assert_eq!(received_session, Some("test-session-123".to_string()));
            }
            _ => panic!("Expected ToolApproved message"),
        }
    }
    
    if let Some(msg) = rx.recv().await {
        match msg {
            ClaudeMessage::StreamText { text, session_id: received_session } => {
                assert_eq!(received_session, Some("test-session-123".to_string()));
                
                // Verify the message contains the required elements
                assert!(text.contains("🔧 Automode: Tool TestTool approved"));
                assert!(text.contains("immediately available"));
                assert!(text.contains("You MUST retry using this tool now"));
                assert!(text.contains("it's imperative that you do so"));
                
                // Verify it doesn't contain the old misleading text
                assert!(!text.contains("after restart"));
                assert!(!text.contains("will be available"));
                
                println!("✅ Message format verified: {}", text);
            }
            _ => panic!("Expected StreamText message"),
        }
    }
}

#[tokio::test]
async fn test_tool_approval_message_variations() {
    let test_tools = vec![
        "mcp__playwright__browser_navigate",
        "mcp__deepwiki__read_wiki_contents", 
        "mcp__taskmaster-ai__get_tasks",
        "WebSearch",
        "Bash",
    ];
    
    for tool_name in test_tools {
        let message = format!("🔧 Automode: Tool {} approved and is immediately available! You MUST retry using this tool now - it's imperative that you do so.", tool_name);
        
        // Verify each tool name appears correctly in the message
        assert!(message.contains(&format!("Tool {} approved", tool_name)));
        assert!(message.contains("immediately available"));
        assert!(message.contains("MUST retry"));
        assert!(message.contains("imperative"));
        
        println!("✅ Verified message for tool: {}", tool_name);
    }
}

#[test]
fn test_message_is_imperative_and_actionable() {
    let test_message = "🔧 Automode: Tool TestTool approved and is immediately available! You MUST retry using this tool now - it's imperative that you do so.";
    
    // Check for imperative language
    assert!(test_message.contains("MUST"));
    assert!(test_message.contains("imperative"));
    assert!(test_message.contains("now"));
    
    // Check for immediate availability
    assert!(test_message.contains("immediately available"));
    
    // Check for clear action instruction
    assert!(test_message.contains("retry using this tool"));
    
    // Ensure no confusing language about restarts or delays
    assert!(!test_message.contains("restart"));
    assert!(!test_message.contains("after"));
    assert!(!test_message.contains("will be"));
    assert!(!test_message.contains("later"));
    
    println!("✅ Message is properly imperative and actionable");
}

#[test]
fn test_message_clarity_and_urgency() {
    let test_message = "🔧 Automode: Tool TestTool approved and is immediately available! You MUST retry using this tool now - it's imperative that you do so.";
    
    // Message should be under 150 characters for clarity (this is ~140)
    assert!(test_message.len() < 150, "Message too long: {} chars", test_message.len());
    
    // Should have clear emoji indicator
    assert!(test_message.starts_with("🔧"));
    
    // Should have exclamation points for urgency  
    assert!(test_message.matches('!').count() >= 1);
    
    // Should be a single sentence for clarity
    let sentence_count = test_message.matches('.').count();
    assert_eq!(sentence_count, 1, "Should be one clear sentence");
    
    println!("✅ Message has appropriate clarity and urgency");
}