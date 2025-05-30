use veda_tui::deepseek::{analyze_claude_message, create_documentation_prompt};

#[test]
fn test_automode_question_detection() {
    // Test various question formats
    let test_cases = vec![
        ("What is the best way to implement this?", true),
        ("How do I use React hooks?", true),
        ("Could you explain this code?", true),
        ("Can you help me with this error?", true),
        ("Where should I put this function?", true),
        ("When does this event fire?", true),
        ("Why is this not working?", true),
        ("Which library should I use?", true),
        ("I need help understanding this", true),
        ("Please explain the architecture", true),
        ("I'm looking for documentation", true),
        ("I'll implement that now.", false),
        ("Thank you for the explanation.", false),
        ("Let me fix that bug.", false),
        ("The code is working now.", false),
    ];

    for (message, expected) in test_cases {
        let (is_question, _) = analyze_claude_message(message);
        assert_eq!(is_question, expected, "Failed for message: {}", message);
    }
}

#[test]
fn test_documentation_detection() {
    // Test cases that should trigger documentation suggestions
    let doc_cases = vec![
        "How do I use the React documentation?",
        "Where can I find the API docs?",
        "What's the library reference for this?",
        "Can you show me an example?",
        "Is there a tutorial for this framework?",
    ];

    for message in doc_cases {
        let (is_question, hint) = analyze_claude_message(message);
        assert!(is_question, "Should be detected as question: {}", message);
        assert!(hint.is_some(), "Should suggest documentation: {}", message);
    }

    // Test cases that shouldn't trigger documentation
    let non_doc_cases = vec![
        "What time is it?",
        "How many items are in the list?",
        "What color should I use?",
    ];

    for message in non_doc_cases {
        let (is_question, hint) = analyze_claude_message(message);
        assert!(is_question, "Should be detected as question: {}", message);
        assert!(hint.is_none(), "Should not suggest documentation: {}", message);
    }
}

#[test]
fn test_documentation_prompt_generation() {
    let topics = vec![
        "React hooks",
        "Vue.js components",
        "Express middleware",
        "Django models",
    ];

    for topic in topics {
        let prompt = create_documentation_prompt(topic);
        
        // Check that prompt contains key elements
        assert!(prompt.contains("deepwiki"), "Should mention deepwiki");
        assert!(prompt.contains(topic), "Should mention the topic: {}", topic);
        assert!(prompt.contains("mcp__deepwiki__read_wiki_structure"), "Should mention structure command");
        assert!(prompt.contains("mcp__deepwiki__read_wiki_contents"), "Should mention contents command");
        assert!(prompt.contains("mcp__deepwiki__ask_question"), "Should mention question command");
        assert!(prompt.contains("owner/repo"), "Should show parameter format");
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_automode_workflow() {
        // Simulate a typical automode workflow
        let claude_messages = vec![
            "How do I implement authentication in React?",
            "What's the best way to handle state management?",
            "Can you show me the documentation for React Router?",
        ];

        for message in claude_messages {
            let (is_question, hint) = analyze_claude_message(message);
            assert!(is_question, "Should detect as question: {}", message);
            
            // For messages containing specific keywords, we should get hints
            if message.contains("documentation") {
                assert!(hint.is_some(), "Should provide documentation hint for: {}", message);
            }
        }
    }
    
    #[tokio::test]
    async fn test_tool_permission_patterns() {
        // Test messages that should be detected as permission issues
        let permission_messages = vec![
            "I'm not allowed to use the deepwiki tool.",
            "I don't have permission to access mcp__deepwiki__read_wiki_contents",
            "The playwright tool is not available to me.",
            "I cannot use the filesystem tools.",
            "Sorry, I can't access the GitHub API.",
        ];
        
        // Test messages that should NOT be detected as permission issues
        let non_permission_messages = vec![
            "I found an error in the code.",
            "The function is not working correctly.",
            "I can help you with that.",
            "Let me analyze this for you.",
        ];
        
        // Note: These would need actual DeepSeek API calls to test properly
        // For unit tests, we're just verifying the patterns would be sent to DeepSeek
        for msg in permission_messages {
            // In a real test, we'd mock the DeepSeek response
            println!("Would check permission issue for: {}", msg);
        }
        
        for msg in non_permission_messages {
            println!("Should not detect permission issue for: {}", msg);
        }
    }
}