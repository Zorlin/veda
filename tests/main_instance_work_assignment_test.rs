use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

// Mock structures to test main instance work assignment
#[derive(Clone)]
struct MockInstance {
    id: Uuid,
    name: String,
    session_id: Option<String>,
    messages: Vec<MockMessage>,
    is_processing: bool,
    working_directory: String,
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
            session_id: None,
            messages: Vec::new(),
            is_processing: false,
            working_directory: "/test/dir".to_string(),
        }
    }
    
    fn add_message(&mut self, sender: String, content: String) {
        self.messages.push(MockMessage { sender, content });
    }
    
    fn start_processing(&mut self) {
        self.is_processing = true;
    }
}

struct MockApp {
    instances: Vec<MockInstance>,
    current_tab: usize,
    max_instances: usize,
    message_queue: Vec<String>,
}

impl MockApp {
    fn new() -> Self {
        Self {
            instances: vec![MockInstance::new("Claude 1".to_string())],
            current_tab: 0,
            max_instances: 5,
            message_queue: Vec::new(),
        }
    }
    
    // Simulate the coordination spawning process
    async fn spawn_coordinated_instances_with_work_assignment(&mut self, main_instance_id: Uuid, breakdown: &str) {
        let subtasks: Vec<&str> = breakdown.lines()
            .filter(|line| line.starts_with("SUBTASK_"))
            .collect();
        
        if subtasks.is_empty() {
            return;
        }
        
        let instances_to_spawn = subtasks.len().min(self.max_instances - self.instances.len());
        
        // Add coordination message to main instance
        if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
            main_instance.add_message("System".to_string(), 
                format!("🤝 Coordinating {} parallel instances for task division", instances_to_spawn));
        }
        
        // Spawn additional instances for each subtask (skipping first one for main instance)
        for i in 1..instances_to_spawn {
            if self.instances.len() >= self.max_instances {
                break;
            }
            
            let subtask = subtasks.get(i).unwrap_or(&"General coordination task");
            let instance_name = format!("Claude {}", self.instances.len() + 1);
            let mut new_instance = MockInstance::new(instance_name);
            
            // Parse subtask details
            let task_parts: Vec<&str> = subtask.split(" | ").collect();
            let task_desc = task_parts.get(0)
                .unwrap_or(&"")
                .trim_start_matches("SUBTASK_")
                .trim_start_matches(&format!("{}: ", i + 1));
            
            let coordination_message = format!(
                "🤝 MULTI-INSTANCE COORDINATION MODE\n\nYOUR ASSIGNED SUBTASK: {}",
                task_desc
            );
            
            new_instance.add_message("System".to_string(), coordination_message);
            new_instance.start_processing();
            self.instances.push(new_instance);
        }
        
        // Assign work to the main instance (this is the key fix)
        if let Some(main_instance) = self.instances.iter_mut().find(|i| i.id == main_instance_id) {
            // Assign the first subtask to main instance
            let first_subtask = subtasks[0];
            let task_parts: Vec<&str> = first_subtask.split(" | ").collect();
            let task_desc = task_parts.get(0)
                .unwrap_or(&"")
                .trim_start_matches("SUBTASK_")
                .trim_start_matches("1: ");
            let scope = task_parts.iter()
                .find(|part| part.starts_with("SCOPE:"))
                .map(|s| s.trim_start_matches("SCOPE:").trim())
                .unwrap_or("Project coordination");
            
            let main_task = format!("YOUR ASSIGNED TASK: {}\nSCOPE: {}", task_desc, scope);
            
            main_instance.add_message("System".to_string(), 
                format!("🎯 MAIN INSTANCE COORDINATION ASSIGNMENT:\n{}\n\n⚡ BEGIN WORKING: Start with your assigned task immediately!", main_task));
            
            // Main instance should start processing its assigned work
            main_instance.start_processing();
            
            // Simulate auto-starting main instance
            let main_task_instruction = format!(
                "Please begin working on your assigned task: {}\n\nScope: {}\n\nStart working immediately!", 
                task_desc, scope
            );
            self.message_queue.push(main_task_instruction);
        }
        
        // Switch back to main instance (Tab 1) so user can see it working
        self.current_tab = 0;
    }
    
    // Simulate sending auto-start messages
    async fn process_auto_start_messages(&mut self) {
        for message in &self.message_queue {
            if let Some(main_instance) = self.instances.get_mut(0) {
                main_instance.add_message("User".to_string(), message.clone());
            }
        }
        self.message_queue.clear();
    }
}

#[tokio::test]
async fn test_main_instance_gets_assigned_work() {
    // Test that the main instance (Claude 1) gets assigned work after spawning
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    // Verify initial state
    assert_eq!(app.instances.len(), 1, "Should start with 1 instance");
    assert!(!app.instances[0].is_processing, "Main instance should not be processing initially");
    
    // Simulate coordination with subtasks
    let breakdown = "SUBTASK_1: Implement Raft consensus algorithm | SCOPE: src/raft/ | PRIORITY: High\nSUBTASK_2: Develop erasure coding system | SCOPE: src/erasure/ | PRIORITY: High\nSUBTASK_3: Create CLI management tools | SCOPE: src/cli/ | PRIORITY: Medium";
    
    app.spawn_coordinated_instances_with_work_assignment(main_instance_id, breakdown).await;
    
    // Process the auto-start messages
    app.process_auto_start_messages().await;
    
    // Verify instances were spawned
    assert_eq!(app.instances.len(), 3, "Should have spawned 2 additional instances (1 main + 2 spawned)");
    
    // Verify main instance got work assignment
    let main_instance = &app.instances[0];
    assert!(main_instance.is_processing, "Main instance should be processing after coordination");
    
    // Check that main instance has coordination and work assignment messages
    let main_messages: Vec<String> = main_instance.messages.iter()
        .map(|m| m.content.clone())
        .collect();
    
    assert!(main_messages.iter().any(|m| m.contains("Coordinating") && m.contains("parallel instances")), 
           "Main instance should have coordination message");
    assert!(main_messages.iter().any(|m| m.contains("MAIN INSTANCE COORDINATION ASSIGNMENT")), 
           "Main instance should have work assignment");
    assert!(main_messages.iter().any(|m| m.contains("Implement Raft consensus algorithm")), 
           "Main instance should be assigned the first subtask");
    assert!(main_messages.iter().any(|m| m.contains("BEGIN WORKING")), 
           "Main instance should be told to begin working");
    assert!(main_messages.iter().any(|m| m.contains("Please begin working on your assigned task")), 
           "Main instance should receive auto-start instruction");
    
    // Verify spawned instances also got work
    for i in 1..app.instances.len() {
        assert!(app.instances[i].is_processing, "Spawned instance {} should be processing", i + 1);
        let spawned_messages: Vec<String> = app.instances[i].messages.iter()
            .map(|m| m.content.clone())
            .collect();
        assert!(spawned_messages.iter().any(|m| m.contains("MULTI-INSTANCE COORDINATION MODE")), 
               "Spawned instance {} should have coordination message", i + 1);
    }
    
    // Verify tab switched back to main instance
    assert_eq!(app.current_tab, 0, "Should switch back to main instance tab");
    
    println!("✅ Main instance gets assigned work after spawning");
}

#[tokio::test]
async fn test_main_instance_gets_first_priority_task() {
    // Test that main instance gets the first/highest priority task
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    // Breakdown with clear priority ordering
    let breakdown = "SUBTASK_1: Critical database migration | SCOPE: src/db/ | PRIORITY: High\nSUBTASK_2: Optional UI improvements | SCOPE: src/ui/ | PRIORITY: Low\nSUBTASK_3: Documentation updates | SCOPE: docs/ | PRIORITY: Medium";
    
    app.spawn_coordinated_instances_with_work_assignment(main_instance_id, breakdown).await;
    app.process_auto_start_messages().await;
    
    // Verify main instance got the high priority task (first subtask)
    let main_messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    
    assert!(main_messages.iter().any(|m| m.contains("Critical database migration")), 
           "Main instance should get the first/highest priority task");
    assert!(main_messages.iter().any(|m| m.contains("src/db/")), 
           "Main instance should get the correct scope");
    
    println!("✅ Main instance gets first priority task");
}

#[tokio::test]
async fn test_main_instance_coordination_role_fallback() {
    // Test that main instance gets coordination role when no subtasks
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    // Breakdown with no valid subtasks
    let breakdown = "SINGLE_INSTANCE_SUFFICIENT: Task is not separable";
    
    app.spawn_coordinated_instances_with_work_assignment(main_instance_id, breakdown).await;
    
    // Should not spawn additional instances
    assert_eq!(app.instances.len(), 1, "Should not spawn instances for non-separable task");
    
    // Main instance should not be assigned subtask work, but should continue normally
    assert!(!app.instances[0].is_processing, "Main instance should not auto-start for non-coordination tasks");
    
    println!("✅ Main instance coordination role fallback works");
}

#[tokio::test]
async fn test_main_instance_work_assignment_timing() {
    // Test that main instance work assignment happens after spawning is complete
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    let breakdown = "SUBTASK_1: Task A | SCOPE: src/a/ | PRIORITY: High\nSUBTASK_2: Task B | SCOPE: src/b/ | PRIORITY: Medium";
    
    // Track message order
    let mut pre_spawn_message_count = app.instances[0].messages.len();
    
    app.spawn_coordinated_instances_with_work_assignment(main_instance_id, breakdown).await;
    
    let post_spawn_message_count = app.instances[0].messages.len();
    
    // Should have added coordination messages
    assert!(post_spawn_message_count > pre_spawn_message_count, 
           "Main instance should have new messages after spawning");
    
    // Check message order
    let messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    
    // Find indices of key messages
    let coordination_msg_idx = messages.iter().position(|m| m.contains("Coordinating") && m.contains("parallel instances"));
    let assignment_msg_idx = messages.iter().position(|m| m.contains("MAIN INSTANCE COORDINATION ASSIGNMENT"));
    
    assert!(coordination_msg_idx.is_some(), "Should have coordination message");
    assert!(assignment_msg_idx.is_some(), "Should have work assignment message");
    assert!(coordination_msg_idx.unwrap() < assignment_msg_idx.unwrap(), 
           "Coordination message should come before work assignment");
    
    println!("✅ Main instance work assignment timing is correct");
}

#[tokio::test]
async fn test_all_instances_get_different_tasks() {
    // Test that main instance and spawned instances get different tasks
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    let breakdown = "SUBTASK_1: Task Alpha | SCOPE: src/alpha/ | PRIORITY: High\nSUBTASK_2: Task Beta | SCOPE: src/beta/ | PRIORITY: High\nSUBTASK_3: Task Gamma | SCOPE: src/gamma/ | PRIORITY: Medium";
    
    app.spawn_coordinated_instances_with_work_assignment(main_instance_id, breakdown).await;
    
    // Verify main instance gets Task Alpha (first subtask)
    let main_messages: Vec<String> = app.instances[0].messages.iter()
        .map(|m| m.content.clone())
        .collect();
    assert!(main_messages.iter().any(|m| m.contains("Task Alpha")), "Main instance should get Task Alpha");
    
    // Verify spawned instances get Beta and Gamma
    if app.instances.len() > 1 {
        let spawned_1_messages: Vec<String> = app.instances[1].messages.iter()
            .map(|m| m.content.clone())
            .collect();
        assert!(spawned_1_messages.iter().any(|m| m.contains("Task Beta")), "Second instance should get Task Beta");
    }
    
    if app.instances.len() > 2 {
        let spawned_2_messages: Vec<String> = app.instances[2].messages.iter()
            .map(|m| m.content.clone())
            .collect();
        assert!(spawned_2_messages.iter().any(|m| m.contains("Task Gamma")), "Third instance should get Task Gamma");
    }
    
    // Verify no task overlap
    assert!(!main_messages.iter().any(|m| m.contains("Task Beta") || m.contains("Task Gamma")), 
           "Main instance should not have other tasks");
    
    println!("✅ All instances get different tasks");
}