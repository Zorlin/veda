use uuid::Uuid;

// Mock structures to test !max command functionality
#[derive(Clone)]
struct MockInstance {
    id: Uuid,
    name: String,
    messages: Vec<MockMessage>,
}

#[derive(Clone, Debug)]
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
        }
    }
    
    fn add_message(&mut self, sender: String, content: String) {
        self.messages.push(MockMessage { sender, content });
    }
}

struct MockApp {
    instances: Vec<MockInstance>,
    current_tab: usize,
    max_instances: usize,
}

impl MockApp {
    fn new() -> Self {
        Self {
            instances: vec![MockInstance::new("Claude 1".to_string())],
            current_tab: 0,
            max_instances: 8, // Default from main app
        }
    }
    
    fn new_with_instances(instance_count: usize) -> Self {
        let mut instances = vec![MockInstance::new("Claude 1".to_string())];
        for i in 2..=instance_count {
            instances.push(MockInstance::new(format!("Claude {}", i)));
        }
        Self {
            instances,
            current_tab: 0,
            max_instances: 8,
        }
    }
    
    fn current_instance_mut(&mut self) -> Option<&mut MockInstance> {
        self.instances.get_mut(self.current_tab)
    }
    
    async fn handle_max_command(&mut self, max_str: &str) {
        // Add user message showing the command first
        if let Some(instance) = self.current_instance_mut() {
            instance.add_message("You".to_string(), format!("!max {}", max_str));
        }
        
        // Parse the max instances value
        match max_str.trim().parse::<usize>() {
            Ok(new_max) if new_max > 0 && new_max <= 20 => {
                let old_max = self.max_instances;
                self.max_instances = new_max;
                
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        format!("⚙️ Max instances changed from {} to {}", old_max, new_max)
                    );
                }
                
                // If we now exceed the limit, schedule excess instances for shutdown
                if self.instances.len() > new_max {
                    let excess_count = self.instances.len() - new_max;
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message(
                            "System".to_string(), 
                            format!("🔄 {} instances exceed the new limit and will shut down after completing current tasks", excess_count)
                        );
                    }
                    
                    // Mark excess instances for shutdown (starting from the end, keeping main instance)
                    for i in (new_max..self.instances.len()).rev() {
                        if i > 0 { // Never shut down the main instance (index 0)
                            if let Some(instance_to_shutdown) = self.instances.get_mut(i) {
                                instance_to_shutdown.add_message(
                                    "System".to_string(), 
                                    "🚪 This instance will shut down after completing current task due to new max limit".to_string()
                                );
                            }
                        }
                    }
                    
                    // Trigger graceful shutdown process
                    self.shutdown_excess_instances().await;
                } else {
                    let instances_len = self.instances.len();
                    if let Some(instance) = self.current_instance_mut() {
                        instance.add_message(
                            "System".to_string(), 
                            format!("✅ Current instance count ({}) is within the new limit", instances_len)
                        );
                    }
                }
            }
            Ok(new_max) if new_max > 20 => {
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        "❌ Maximum instance limit cannot exceed 20".to_string()
                    );
                }
            }
            Ok(_) => {
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        "❌ Maximum instance limit must be at least 1".to_string()
                    );
                }
            }
            Err(_) => {
                if let Some(instance) = self.current_instance_mut() {
                    instance.add_message(
                        "System".to_string(), 
                        format!("❌ Invalid number format: '{}'. Usage: !max <number>", max_str)
                    );
                }
            }
        }
    }
    
    async fn shutdown_excess_instances(&mut self) {
        // Identify instances that should be shut down (beyond max_instances limit)
        if self.instances.len() <= self.max_instances {
            return; // No excess instances
        }
        
        let instances_to_remove = self.instances.len() - self.max_instances;
        
        // Remove excess instances from the end (keep main instance at index 0)
        let mut removed_count = 0;
        while self.instances.len() > self.max_instances && removed_count < instances_to_remove {
            let last_index = self.instances.len() - 1;
            if last_index > 0 { // Never remove the main instance
                self.instances.remove(last_index);
                
                // If we were on the removed tab, switch to the previous tab
                if self.current_tab >= self.instances.len() {
                    self.current_tab = self.instances.len().saturating_sub(1);
                }
                
                removed_count += 1;
            } else {
                break; // Don't remove the main instance
            }
        }
        
        // Log the new state
        let instances_len = self.instances.len();
        let max_instances = self.max_instances;
        if let Some(instance) = self.current_instance_mut() {
            instance.add_message(
                "System".to_string(), 
                format!("✅ Successfully shut down {} excess instances. Current count: {}/{}", 
                       removed_count, instances_len, max_instances)
            );
        }
    }
    
    // Helper method to simulate spawning instances
    fn spawn_instances(&mut self, count: usize) {
        for _i in 0..count {
            if self.instances.len() < self.max_instances {
                let instance_name = format!("Claude {}", self.instances.len() + 1);
                self.instances.push(MockInstance::new(instance_name));
            }
        }
    }
}

#[tokio::test]
async fn test_max_command_increase_limit() {
    // Test increasing the max instances limit
    let mut app = MockApp::new();
    
    // Verify initial state
    assert_eq!(app.max_instances, 8);
    assert_eq!(app.instances.len(), 1);
    
    // Increase max instances to 12
    app.handle_max_command("12").await;
    
    // Verify the limit was increased
    assert_eq!(app.max_instances, 12);
    
    // Check system messages
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    
    assert!(messages.iter().any(|m| m.contains("!max 12")), "Should contain user command");
    assert!(messages.iter().any(|m| m.contains("Max instances changed from 8 to 12")), "Should show limit change");
    assert!(messages.iter().any(|m| m.contains("Current instance count (1) is within the new limit")), "Should confirm within limit");
    
    println!("✅ Max command increase limit works correctly");
}

#[tokio::test]
async fn test_max_command_decrease_limit_with_shutdown() {
    // Test decreasing the max instances limit when it requires shutdowns
    let mut app = MockApp::new_with_instances(6); // Start with 6 instances
    
    // Verify initial state
    assert_eq!(app.max_instances, 8);
    assert_eq!(app.instances.len(), 6);
    
    // Decrease max instances to 3
    app.handle_max_command("3").await;
    
    // Verify the limit was decreased and instances were shut down
    assert_eq!(app.max_instances, 3);
    assert_eq!(app.instances.len(), 3); // Should be reduced to 3
    
    // Check that main instance is still there
    assert_eq!(app.instances[0].name, "Claude 1");
    
    // Check system messages in main instance
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    
    assert!(messages.iter().any(|m| m.contains("!max 3")), "Should contain user command");
    assert!(messages.iter().any(|m| m.contains("Max instances changed from 8 to 3")), "Should show limit change");
    assert!(messages.iter().any(|m| m.contains("3 instances exceed the new limit")), "Should indicate excess instances");
    assert!(messages.iter().any(|m| m.contains("Successfully shut down 3 excess instances")), "Should confirm shutdown");
    
    println!("✅ Max command decrease with shutdown works correctly");
}

#[tokio::test]
async fn test_max_command_invalid_inputs() {
    let mut app = MockApp::new();
    
    // Test invalid number format
    app.handle_max_command("invalid").await;
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(messages.iter().any(|m| m.contains("Invalid number format: 'invalid'")), "Should handle invalid format");
    
    // Clear messages for next test
    app.instances[0].messages.clear();
    
    // Test zero value
    app.handle_max_command("0").await;
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(messages.iter().any(|m| m.contains("Maximum instance limit must be at least 1")), "Should reject zero");
    
    // Clear messages for next test
    app.instances[0].messages.clear();
    
    // Test value exceeding maximum
    app.handle_max_command("25").await;
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(messages.iter().any(|m| m.contains("Maximum instance limit cannot exceed 20")), "Should reject values > 20");
    
    // Verify max_instances wasn't changed for invalid inputs
    assert_eq!(app.max_instances, 8);
    
    println!("✅ Max command handles invalid inputs correctly");
}

#[tokio::test]
async fn test_max_command_boundary_values() {
    let mut app = MockApp::new();
    
    // Test minimum valid value (1)
    app.handle_max_command("1").await;
    assert_eq!(app.max_instances, 1);
    
    // Test maximum valid value (20)
    app.handle_max_command("20").await;
    assert_eq!(app.max_instances, 20);
    
    // Test value just below maximum (19)
    app.handle_max_command("19").await;
    assert_eq!(app.max_instances, 19);
    
    // Test value just above minimum (2)
    app.handle_max_command("2").await;
    assert_eq!(app.max_instances, 2);
    
    println!("✅ Max command handles boundary values correctly");
}

#[tokio::test]
async fn test_shutdown_never_removes_main_instance() {
    // Test that the main instance (index 0) is never removed
    let mut app = MockApp::new_with_instances(5); // Start with 5 instances
    
    // Set max to 1 (only main instance should remain)
    app.handle_max_command("1").await;
    
    // Verify only main instance remains
    assert_eq!(app.instances.len(), 1);
    assert_eq!(app.instances[0].name, "Claude 1"); // Main instance should remain
    assert_eq!(app.max_instances, 1);
    
    println!("✅ Shutdown never removes main instance");
}

#[tokio::test]
async fn test_current_tab_adjustment_after_shutdown() {
    // Test that current_tab is adjusted when instances are shut down
    let mut app = MockApp::new_with_instances(5); // Start with 5 instances
    app.current_tab = 4; // Switch to last tab (index 4)
    
    // Reduce max instances to 3
    app.handle_max_command("3").await;
    
    // Verify instances were reduced
    assert_eq!(app.instances.len(), 3);
    
    // Verify current_tab was adjusted to a valid index
    assert!(app.current_tab < app.instances.len());
    assert_eq!(app.current_tab, 2); // Should be last valid tab
    
    println!("✅ Current tab adjustment after shutdown works correctly");
}

#[tokio::test]
async fn test_max_command_no_change_when_equal() {
    // Test behavior when setting max to current value
    let mut app = MockApp::new();
    
    // Set max to the same value (8)
    app.handle_max_command("8").await;
    
    // Verify max_instances stayed the same
    assert_eq!(app.max_instances, 8);
    
    // Check that appropriate message was shown
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(messages.iter().any(|m| m.contains("Max instances changed from 8 to 8")), "Should show change message");
    
    println!("✅ Max command handles no change scenario correctly");
}

#[tokio::test]
async fn test_coordination_respects_new_max_limit() {
    // Test that coordination logic respects the new max limit
    let mut app = MockApp::new();
    
    // Simulate having some instances already
    app.spawn_instances(3); // Now have 4 total (1 + 3)
    assert_eq!(app.instances.len(), 4);
    
    // Set max to 5
    app.handle_max_command("5").await;
    assert_eq!(app.max_instances, 5);
    
    // Try to spawn more instances (should be limited by new max)
    app.spawn_instances(5); // Try to spawn 5 more
    
    // Should only have 5 total (limited by max_instances)
    assert_eq!(app.instances.len(), 5);
    
    println!("✅ Coordination respects new max limit");
}

#[tokio::test]
async fn test_shutdown_excess_instances_order() {
    // Test that instances are shut down in the correct order (last created first)
    let mut app = MockApp::new_with_instances(6); // Claude 1, Claude 2, Claude 3, Claude 4, Claude 5, Claude 6
    
    // Verify initial order
    assert_eq!(app.instances[0].name, "Claude 1");
    assert_eq!(app.instances[5].name, "Claude 6");
    
    // Set max to 3 (should remove Claude 6, Claude 5, Claude 4)
    app.handle_max_command("3").await;
    
    // Verify correct instances remain
    assert_eq!(app.instances.len(), 3);
    assert_eq!(app.instances[0].name, "Claude 1");
    assert_eq!(app.instances[1].name, "Claude 2");
    assert_eq!(app.instances[2].name, "Claude 3");
    
    println!("✅ Shutdown removes instances in correct order (LIFO)");
}

#[tokio::test]
async fn test_max_command_with_whitespace() {
    // Test that the command handles whitespace correctly
    let mut app = MockApp::new();
    
    // Test with leading/trailing whitespace
    app.handle_max_command("  5  ").await;
    assert_eq!(app.max_instances, 5);
    
    // Clear messages
    app.instances[0].messages.clear();
    
    // Test with tabs
    app.handle_max_command("\t10\t").await;
    assert_eq!(app.max_instances, 10);
    
    println!("✅ Max command handles whitespace correctly");
}