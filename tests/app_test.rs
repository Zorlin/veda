use veda_tui::ClaudeInstance;
use arboard::Clipboard;
use std::sync::{Arc, Mutex};

struct TestApp {
    instances: Vec<ClaudeInstance>,
    current_tab: usize,
    auto_mode: bool,
    clipboard: Arc<Mutex<Clipboard>>,
}

impl TestApp {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut instances = Vec::new();
        instances.push(ClaudeInstance::new("Test 1".to_string()));
        
        Ok(Self {
            instances,
            current_tab: 0,
            auto_mode: false,
            clipboard: Arc::new(Mutex::new(Clipboard::new()?)),
        })
    }
    
    fn add_instance(&mut self) {
        let instance_num = self.instances.len() + 1;
        self.instances.push(ClaudeInstance::new(format!("Test {}", instance_num)));
        self.current_tab = self.instances.len() - 1;
    }
    
    fn next_tab(&mut self) {
        if !self.instances.is_empty() {
            self.current_tab = (self.current_tab + 1) % self.instances.len();
        }
    }
    
    fn previous_tab(&mut self) {
        if !self.instances.is_empty() {
            self.current_tab = if self.current_tab == 0 {
                self.instances.len() - 1
            } else {
                self.current_tab - 1
            };
        }
    }
    
    fn toggle_auto_mode(&mut self) {
        self.auto_mode = !self.auto_mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_app_creation() {
        let app = TestApp::new().unwrap();
        assert_eq!(app.instances.len(), 1);
        assert_eq!(app.current_tab, 0);
        assert!(!app.auto_mode);
    }
    
    #[test]
    fn test_add_instance() {
        let mut app = TestApp::new().unwrap();
        assert_eq!(app.instances.len(), 1);
        
        app.add_instance();
        assert_eq!(app.instances.len(), 2);
        assert_eq!(app.current_tab, 1);
        assert_eq!(app.instances[1].name, "Test 2");
        
        app.add_instance();
        assert_eq!(app.instances.len(), 3);
        assert_eq!(app.current_tab, 2);
        assert_eq!(app.instances[2].name, "Test 3");
    }
    
    #[test]
    fn test_tab_navigation() {
        let mut app = TestApp::new().unwrap();
        app.add_instance();
        app.add_instance();
        // Now we have 3 tabs: 0, 1, 2
        app.current_tab = 0;
        
        app.next_tab();
        assert_eq!(app.current_tab, 1);
        
        app.next_tab();
        assert_eq!(app.current_tab, 2);
        
        app.next_tab();
        assert_eq!(app.current_tab, 0); // Wraps around
        
        app.previous_tab();
        assert_eq!(app.current_tab, 2); // Wraps around
        
        app.previous_tab();
        assert_eq!(app.current_tab, 1);
        
        app.previous_tab();
        assert_eq!(app.current_tab, 0);
    }
    
    #[test]
    fn test_toggle_auto_mode() {
        let mut app = TestApp::new().unwrap();
        assert!(!app.auto_mode);
        
        app.toggle_auto_mode();
        assert!(app.auto_mode);
        
        app.toggle_auto_mode();
        assert!(!app.auto_mode);
    }
    
    #[test]
    fn test_single_tab_navigation() {
        let mut app = TestApp::new().unwrap();
        // Only one tab
        assert_eq!(app.current_tab, 0);
        
        app.next_tab();
        assert_eq!(app.current_tab, 0); // Stays at 0
        
        app.previous_tab();
        assert_eq!(app.current_tab, 0); // Stays at 0
    }
    
    #[tokio::test]
    async fn test_message_processing() {
        let mut instance = ClaudeInstance::new("Test".to_string());
        let _id = instance.id;
        
        // Simulate receiving messages
        instance.add_message("You".to_string(), "Hello Claude".to_string());
        assert_eq!(instance.messages.len(), 1);
        assert!(!instance.is_processing);
        
        // Simulate processing
        instance.is_processing = true;
        instance.add_message("Claude".to_string(), String::new());
        assert_eq!(instance.messages.len(), 2);
        
        // Simulate streaming text
        if let Some(last_msg) = instance.messages.last_mut() {
            last_msg.content.push_str("Hello! ");
            last_msg.content.push_str("How can I help you today?");
        }
        
        instance.is_processing = false;
        
        assert_eq!(instance.messages[1].content, "Hello! How can I help you today?");
    }
}