pub mod claude;
pub mod deepseek;
pub mod shared_ipc;

use chrono::Local;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub timestamp: String,
    pub sender: String,
    pub content: String,
    pub is_thinking: bool,
    pub is_collapsed: bool,
}

#[derive(Debug)]
pub struct ClaudeInstance {
    pub id: Uuid,
    pub name: String,
    pub messages: Vec<Message>,
    pub input_buffer: String,
    pub is_processing: bool,
    // Text selection state
    pub selection_start: Option<(u16, u16)>,
    pub selection_end: Option<(u16, u16)>,
    pub selecting: bool,
    pub scroll_offset: u16,
    pub last_tool_attempts: Vec<String>,
    pub session_id: Option<String>,
}

impl ClaudeInstance {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            messages: Vec::new(),
            input_buffer: String::new(),
            is_processing: false,
            selection_start: None,
            selection_end: None,
            selecting: false,
            scroll_offset: 0,
            last_tool_attempts: Vec::new(),
            session_id: None,
        }
    }

    pub fn add_message(&mut self, sender: String, content: String) {
        let timestamp = Local::now().format("%H:%M:%S").to_string();
        self.messages.push(Message {
            timestamp,
            sender,
            content,
            is_thinking: false,
            is_collapsed: false,
        });
    }

    pub fn get_selected_text(&self) -> Option<String> {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let mut selected_lines = Vec::new();
            let start_y = start.1.min(end.1) as usize;
            let end_y = start.1.max(end.1) as usize;
            
            for (i, msg) in self.messages.iter().enumerate() {
                if i >= start_y && i <= end_y {
                    selected_lines.push(format!("{} {}: {}", msg.timestamp, msg.sender, msg.content));
                }
            }
            
            if !selected_lines.is_empty() {
                return Some(selected_lines.join("\n"));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_instance_new() {
        let instance = ClaudeInstance::new("Test".to_string());
        assert_eq!(instance.name, "Test");
        assert_eq!(instance.messages.len(), 0);
        assert_eq!(instance.input_buffer, "");
        assert!(!instance.is_processing);
        assert!(instance.selection_start.is_none());
        assert!(instance.selection_end.is_none());
        assert!(!instance.selecting);
        assert_eq!(instance.scroll_offset, 0);
    }

    #[test]
    fn test_add_message() {
        let mut instance = ClaudeInstance::new("Test".to_string());
        instance.add_message("User".to_string(), "Hello".to_string());
        instance.add_message("Claude".to_string(), "Hi there!".to_string());
        
        assert_eq!(instance.messages.len(), 2);
        assert_eq!(instance.messages[0].sender, "User");
        assert_eq!(instance.messages[0].content, "Hello");
        assert_eq!(instance.messages[1].sender, "Claude");
        assert_eq!(instance.messages[1].content, "Hi there!");
    }

    #[test]
    fn test_get_selected_text_no_selection() {
        let instance = ClaudeInstance::new("Test".to_string());
        assert!(instance.get_selected_text().is_none());
    }

    #[test]
    fn test_get_selected_text_single_line() {
        let mut instance = ClaudeInstance::new("Test".to_string());
        instance.add_message("User".to_string(), "Hello".to_string());
        instance.add_message("Claude".to_string(), "Hi there!".to_string());
        
        instance.selection_start = Some((0, 0));
        instance.selection_end = Some((10, 0));
        
        let selected = instance.get_selected_text().unwrap();
        assert!(selected.contains("User: Hello"));
    }

    #[test]
    fn test_get_selected_text_multiple_lines() {
        let mut instance = ClaudeInstance::new("Test".to_string());
        instance.add_message("User".to_string(), "Hello".to_string());
        instance.add_message("Claude".to_string(), "Hi there!".to_string());
        instance.add_message("User".to_string(), "How are you?".to_string());
        
        instance.selection_start = Some((0, 0));
        instance.selection_end = Some((10, 2));
        
        let selected = instance.get_selected_text().unwrap();
        assert!(selected.contains("User: Hello"));
        assert!(selected.contains("Claude: Hi there!"));
        assert!(selected.contains("User: How are you?"));
    }

    #[test]
    fn test_message_equality() {
        let msg1 = Message {
            timestamp: "12:00:00".to_string(),
            sender: "User".to_string(),
            content: "Hello".to_string(),
            is_thinking: false,
            is_collapsed: false,
        };
        
        let msg2 = Message {
            timestamp: "12:00:00".to_string(),
            sender: "User".to_string(),
            content: "Hello".to_string(),
            is_thinking: false,
            is_collapsed: false,
        };
        
        assert_eq!(msg1, msg2);
    }
}