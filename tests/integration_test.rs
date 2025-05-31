#[cfg(test)]
mod tests {

    #[test]
    fn test_message_creation() {
        // Test that we can create messages with proper timestamps
        let msg = veda_tui::Message {
            timestamp: "12:34:56".to_string(),
            sender: "Test".to_string(),
            content: "Hello, world!".to_string(),
            is_thinking: false,
            is_collapsed: false,
        };
        
        assert_eq!(msg.sender, "Test");
        assert_eq!(msg.content, "Hello, world!");
    }

    #[test]
    fn test_claude_instance_creation() {
        let instance = veda_tui::ClaudeInstance::new("Test Instance".to_string());
        assert_eq!(instance.name, "Test Instance");
        assert!(instance.messages.is_empty());
        assert!(!instance.is_processing);
    }
}