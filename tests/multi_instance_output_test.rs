use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use uuid::Uuid;

// Mock structures to test multi-instance output functionality
#[derive(Clone)]
struct MockInstance {
    id: Uuid,
    name: String,
    messages: Vec<MockMessage>,
    working_directory: String,
    is_processing: bool,
    has_output: bool,
}

#[derive(Clone, Debug)]
struct MockMessage {
    sender: String,
    content: String,
    timestamp: std::time::SystemTime,
}

impl MockInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            messages: Vec::new(),
            working_directory: "/test/working/dir".to_string(),
            is_processing: false,
            has_output: false,
        }
    }
    
    fn add_message(&mut self, sender: String, content: String) {
        self.messages.push(MockMessage {
            sender,
            content,
            timestamp: std::time::SystemTime::now(),
        });
        self.has_output = true;
    }
    
    fn start_processing(&mut self) {
        self.is_processing = true;
        self.add_message("System".to_string(), "Started processing...".to_string());
    }
    
    fn add_output(&mut self, output: String) {
        self.add_message("Claude".to_string(), output);
    }
    
    fn finish_processing(&mut self) {
        self.is_processing = false;
        self.add_message("System".to_string(), "Processing completed.".to_string());
    }
}

struct MockApp {
    instances: Vec<MockInstance>,
    current_tab: usize,
    coordination_in_progress: bool,
    max_instances: usize,
}

impl MockApp {
    fn new() -> Self {
        Self {
            instances: vec![MockInstance::new("Claude 1".to_string())],
            current_tab: 0,
            coordination_in_progress: false,
            max_instances: 5,
        }
    }
    
    // Simulate spawning multiple instances with coordination
    async fn spawn_multiple_instances(&mut self, task_description: String, num_instances: u8) -> bool {
        if self.coordination_in_progress {
            return false;
        }
        
        self.coordination_in_progress = true;
        
        // Simulate DeepSeek analysis and breakdown
        let breakdown = self.simulate_task_breakdown(&task_description).await;
        
        if breakdown.contains("SUBTASK_") {
            let subtasks: Vec<&str> = breakdown.lines()
                .filter(|line| line.starts_with("SUBTASK_"))
                .collect();
            
            // Spawn instances for each subtask
            for (i, subtask) in subtasks.iter().enumerate().take(num_instances as usize) {
                if self.instances.len() >= self.max_instances {
                    break;
                }
                
                let instance_name = format!("Claude {}", self.instances.len() + 1);
                let mut new_instance = MockInstance::new(instance_name);
                
                // Parse subtask details
                let task_parts: Vec<&str> = subtask.split(" | ").collect();
                let task_desc = task_parts.get(0)
                    .unwrap_or(&"")
                    .trim_start_matches("SUBTASK_")
                    .trim_start_matches(&format!("{}: ", i + 1));
                
                let coordination_message = format!(
                    "COORDINATION: Assigned task: {} | Instance: {}",
                    task_desc, new_instance.name
                );
                
                new_instance.add_message("System".to_string(), coordination_message);
                self.instances.push(new_instance);
            }
            
            self.coordination_in_progress = false;
            return true;
        }
        
        self.coordination_in_progress = false;
        false
    }
    
    async fn simulate_task_breakdown(&self, task_description: &str) -> String {
        sleep(Duration::from_millis(10)).await;
        
        if task_description.contains("parallel") || task_description.contains("multiple") {
            "SUBTASK_1: Implement Raft consensus algorithm | SCOPE: src/raft/ | PRIORITY: High\nSUBTASK_2: Develop erasure coding system | SCOPE: src/erasure/ | PRIORITY: High\nSUBTASK_3: Create CLI management tools | SCOPE: src/cli/ | PRIORITY: Medium".to_string()
        } else {
            "SINGLE_INSTANCE_SUFFICIENT: Task is not separable".to_string()
        }
    }
    
    // Simulate work being done across all instances
    async fn simulate_parallel_work(&mut self) {
        for instance in &mut self.instances {
            if instance.name != "Claude 1" { // Skip main instance for this test
                instance.start_processing();
                
                // Simulate different types of work based on assigned task
                if instance.messages.iter().any(|m| m.content.contains("Raft")) {
                    instance.add_output("Implementing Raft leader election...".to_string());
                    sleep(Duration::from_millis(5)).await;
                    instance.add_output("Raft consensus algorithm completed.".to_string());
                } else if instance.messages.iter().any(|m| m.content.contains("erasure")) {
                    instance.add_output("Setting up Reed-Solomon encoding...".to_string());
                    sleep(Duration::from_millis(5)).await;
                    instance.add_output("Erasure coding system implemented.".to_string());
                } else if instance.messages.iter().any(|m| m.content.contains("CLI")) {
                    instance.add_output("Creating command-line interface...".to_string());
                    sleep(Duration::from_millis(5)).await;
                    instance.add_output("CLI tools implementation complete.".to_string());
                }
                
                instance.finish_processing();
            }
        }
    }
    
    fn get_instance_output(&self, instance_index: usize) -> Option<Vec<String>> {
        self.instances.get(instance_index).map(|instance| {
            instance.messages.iter()
                .map(|msg| format!("{}: {}", msg.sender, msg.content))
                .collect()
        })
    }
    
    fn switch_to_tab(&mut self, tab_index: usize) -> bool {
        if tab_index < self.instances.len() {
            self.current_tab = tab_index;
            true
        } else {
            false
        }
    }
}

#[tokio::test]
async fn test_multiple_instances_spawn_successfully() {
    let mut app = MockApp::new();
    
    // Verify initial state
    assert_eq!(app.instances.len(), 1, "Should start with 1 instance");
    assert_eq!(app.instances[0].name, "Claude 1", "Main instance should be Claude 1");
    
    // Spawn multiple instances
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    let spawn_result = app.spawn_multiple_instances(task_description, 3).await;
    
    // Verify instances were spawned successfully
    assert!(spawn_result, "Should successfully spawn multiple instances");
    assert_eq!(app.instances.len(), 4, "Should have 4 total instances (1 main + 3 spawned)");
    
    // Verify instance names
    assert_eq!(app.instances[0].name, "Claude 1");
    assert_eq!(app.instances[1].name, "Claude 2");
    assert_eq!(app.instances[2].name, "Claude 3");
    assert_eq!(app.instances[3].name, "Claude 4");
    
    // Verify each instance has coordination message
    for i in 1..app.instances.len() {
        assert!(!app.instances[i].messages.is_empty(), "Instance {} should have coordination message", i + 1);
        assert!(app.instances[i].messages[0].content.contains("COORDINATION"), 
               "Instance {} should have coordination message", i + 1);
    }
    
    println!("✅ Multiple instances spawn successfully");
}

#[tokio::test]
async fn test_output_appears_in_all_tabs() {
    let mut app = MockApp::new();
    
    // Spawn multiple instances
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    assert!(app.spawn_multiple_instances(task_description, 3).await, "Should spawn instances");
    
    // Simulate parallel work across all instances
    app.simulate_parallel_work().await;
    
    // Test that each tab has output
    for i in 0..app.instances.len() {
        let output = app.get_instance_output(i);
        assert!(output.is_some(), "Tab {} should have output", i);
        
        let messages = output.unwrap();
        if i == 0 {
            // Main instance might not have work output, just check it exists
            // Main instance starts empty, that's expected (no assertion needed)
        } else {
            // Spawned instances should have coordination message + work output
            assert!(messages.len() >= 3, "Instance {} should have multiple messages", i + 1);
            assert!(messages.iter().any(|m| m.contains("COORDINATION")), 
                   "Instance {} should have coordination message", i + 1);
            assert!(messages.iter().any(|m| m.contains("Claude:")), 
                   "Instance {} should have Claude output", i + 1);
            assert!(messages.iter().any(|m| m.contains("completed") || m.contains("complete")), 
                   "Instance {} should have completion message", i + 1);
        }
    }
    
    println!("✅ Output appears in all tabs");
}

#[tokio::test]
async fn test_tab_switching_with_output_verification() {
    let mut app = MockApp::new();
    
    // Spawn instances and do work
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    assert!(app.spawn_multiple_instances(task_description, 3).await, "Should spawn instances");
    app.simulate_parallel_work().await;
    
    // Test switching to each tab and verifying output
    for tab_index in 0..app.instances.len() {
        assert!(app.switch_to_tab(tab_index), "Should switch to tab {}", tab_index);
        assert_eq!(app.current_tab, tab_index, "Current tab should be {}", tab_index);
        
        // Verify the tab has the expected instance
        let instance = &app.instances[tab_index];
        assert!(instance.has_output || tab_index == 0, "Tab {} should have output or be main instance", tab_index);
        
        if tab_index > 0 {
            // Spawned instances should have specific task-related output
            let messages: Vec<String> = instance.messages.iter()
                .map(|m| m.content.clone())
                .collect();
            
            assert!(messages.iter().any(|m| m.contains("COORDINATION")), 
                   "Tab {} should have coordination message", tab_index);
        }
    }
    
    // Test switching to invalid tab
    assert!(!app.switch_to_tab(10), "Should not switch to invalid tab");
    assert_eq!(app.current_tab, app.instances.len() - 1, "Current tab should remain unchanged");
    
    println!("✅ Tab switching with output verification works");
}

#[tokio::test]
async fn test_concurrent_instance_output() {
    let mut app = MockApp::new();
    
    // Spawn multiple instances
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    assert!(app.spawn_multiple_instances(task_description, 3).await, "Should spawn instances");
    
    // Simulate concurrent work - each instance working on different tasks
    let app_mutex = Arc::new(Mutex::new(app));
    let mut handles = Vec::new();
    
    // Spawn concurrent tasks for each instance (skipping main instance)
    for instance_idx in 1..4 {
        let app_clone = app_mutex.clone();
        let handle = tokio::spawn(async move {
            {
                let mut app = app_clone.lock().await;
                if let Some(instance) = app.instances.get_mut(instance_idx) {
                    instance.start_processing();
                }
            } // Release lock
            
            // Do work outside the lock to avoid deadlocks
            sleep(Duration::from_millis(5)).await;
            
            {
                let mut app = app_clone.lock().await;
                if let Some(instance) = app.instances.get_mut(instance_idx) {
                    // Simulate work based on instance index
                    match instance_idx {
                        1 => {
                            instance.add_output("Raft: Initializing leader election...".to_string());
                            instance.add_output("Raft: Leader elected successfully".to_string());
                        },
                        2 => {
                            instance.add_output("Erasure: Setting up Reed-Solomon matrix...".to_string());
                            instance.add_output("Erasure: Coding matrix ready".to_string());
                        },
                        3 => {
                            instance.add_output("CLI: Parsing command arguments...".to_string());
                            instance.add_output("CLI: Command parser implemented".to_string());
                        },
                        _ => {}
                    }
                    
                    instance.finish_processing();
                }
            }
        });
        handles.push(handle);
    }
    
    // Wait for all concurrent work to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify all instances have their specific output
    let app = app_mutex.lock().await;
    
    // Check Raft instance (index 1)
    let raft_messages: Vec<String> = app.instances[1].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(raft_messages.iter().any(|m| m.contains("Raft")), "Raft instance should have Raft-specific output");
    assert!(raft_messages.iter().any(|m| m.contains("elected successfully")), "Raft instance should complete work");
    
    // Check Erasure instance (index 2)
    let erasure_messages: Vec<String> = app.instances[2].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(erasure_messages.iter().any(|m| m.contains("Erasure")), "Erasure instance should have erasure-specific output");
    assert!(erasure_messages.iter().any(|m| m.contains("matrix ready")), "Erasure instance should complete work");
    
    // Check CLI instance (index 3)
    let cli_messages: Vec<String> = app.instances[3].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(cli_messages.iter().any(|m| m.contains("CLI")), "CLI instance should have CLI-specific output");
    assert!(cli_messages.iter().any(|m| m.contains("parser implemented")), "CLI instance should complete work");
    
    // Verify all instances have completed their processing
    for i in 1..4 {
        assert!(!app.instances[i].is_processing, "Instance {} should have finished processing", i + 1);
        assert!(app.instances[i].has_output, "Instance {} should have output", i + 1);
    }
    
    println!("✅ Concurrent instance output works correctly");
}

#[tokio::test]
async fn test_max_instances_with_output() {
    let mut app = MockApp::new();
    app.max_instances = 3; // Set limit for testing
    
    // Try to spawn more instances than the limit
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools, authentication, database".to_string();
    assert!(app.spawn_multiple_instances(task_description, 5).await, "Should succeed but respect limits");
    
    // Should only have 3 instances total (1 main + 2 spawned due to limit)
    assert_eq!(app.instances.len(), 3, "Should respect max_instances limit");
    
    // Simulate work on all instances
    app.simulate_parallel_work().await;
    
    // Verify all spawned instances have output
    for i in 1..app.instances.len() {
        assert!(app.instances[i].has_output, "Instance {} should have output", i + 1);
        assert!(!app.instances[i].messages.is_empty(), "Instance {} should have messages", i + 1);
    }
    
    println!("✅ Max instances limit with output works correctly");
}

#[tokio::test]
async fn test_output_persistence_across_tab_switches() {
    let mut app = MockApp::new();
    
    // Spawn instances and generate output
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    assert!(app.spawn_multiple_instances(task_description, 3).await, "Should spawn instances");
    app.simulate_parallel_work().await;
    
    // Capture initial output for each tab
    let mut initial_outputs = HashMap::new();
    for i in 0..app.instances.len() {
        initial_outputs.insert(i, app.get_instance_output(i).unwrap());
    }
    
    // Switch between tabs multiple times
    for _ in 0..5 {
        for tab_index in 0..app.instances.len() {
            assert!(app.switch_to_tab(tab_index), "Should switch to tab {}", tab_index);
            
            // Verify output is still there after tab switch
            let current_output = app.get_instance_output(tab_index).unwrap();
            let initial_output = initial_outputs.get(&tab_index).unwrap();
            
            assert_eq!(current_output.len(), initial_output.len(), 
                      "Output length should persist for tab {}", tab_index);
            
            // Verify specific messages are still there
            for (i, msg) in initial_output.iter().enumerate() {
                assert_eq!(&current_output[i], msg, 
                          "Message {} should persist in tab {}", i, tab_index);
            }
        }
    }
    
    println!("✅ Output persistence across tab switches works correctly");
}

#[tokio::test]
async fn test_no_output_interference_between_instances() {
    let mut app = MockApp::new();
    
    // Spawn instances
    let task_description = "Implement multiple parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    assert!(app.spawn_multiple_instances(task_description, 3).await, "Should spawn instances");
    
    // Add specific output to each instance
    app.instances[1].add_output("Raft-specific message A".to_string());
    app.instances[1].add_output("Raft-specific message B".to_string());
    
    app.instances[2].add_output("Erasure-specific message A".to_string());
    app.instances[2].add_output("Erasure-specific message B".to_string());
    
    app.instances[3].add_output("CLI-specific message A".to_string());
    app.instances[3].add_output("CLI-specific message B".to_string());
    
    // Verify no cross-contamination of messages
    let raft_output = app.get_instance_output(1).unwrap();
    let erasure_output = app.get_instance_output(2).unwrap();
    let cli_output = app.get_instance_output(3).unwrap();
    
    // Raft instance should only have Raft messages
    assert!(raft_output.iter().any(|m| m.contains("Raft-specific message A")), 
           "Raft instance should have its own messages");
    assert!(!raft_output.iter().any(|m| m.contains("Erasure-specific")), 
           "Raft instance should not have Erasure messages");
    assert!(!raft_output.iter().any(|m| m.contains("CLI-specific")), 
           "Raft instance should not have CLI messages");
    
    // Erasure instance should only have Erasure messages
    assert!(erasure_output.iter().any(|m| m.contains("Erasure-specific message A")), 
           "Erasure instance should have its own messages");
    assert!(!erasure_output.iter().any(|m| m.contains("Raft-specific")), 
           "Erasure instance should not have Raft messages");
    assert!(!erasure_output.iter().any(|m| m.contains("CLI-specific")), 
           "Erasure instance should not have CLI messages");
    
    // CLI instance should only have CLI messages
    assert!(cli_output.iter().any(|m| m.contains("CLI-specific message A")), 
           "CLI instance should have its own messages");
    assert!(!cli_output.iter().any(|m| m.contains("Raft-specific")), 
           "CLI instance should not have Raft messages");
    assert!(!cli_output.iter().any(|m| m.contains("Erasure-specific")), 
           "CLI instance should not have Erasure messages");
    
    println!("✅ No output interference between instances");
}