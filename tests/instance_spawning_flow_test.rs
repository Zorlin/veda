use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;
use serde_json;

// Mock the ClaudeMessage enum for testing
#[derive(Debug, Clone)]
enum MockClaudeMessage {
    VedaSpawnInstances { 
        instance_id: Uuid, 
        task_description: String, 
        num_instances: u8 
    },
    InternalCoordinateInstances { 
        main_instance_id: Uuid, 
        task_description: String, 
        num_instances: usize, 
        working_dir: String,
        is_ipc: bool,
    },
    StreamText { 
        instance_id: Uuid, 
        text: String 
    },
}

#[derive(Clone)]
struct MockInstance {
    id: Uuid,
    name: String,
    working_directory: String,
    messages: Vec<String>,
}

impl MockInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            working_directory: "/test/working/dir".to_string(),
            messages: Vec::new(),
        }
    }
    
    fn add_message(&mut self, sender: String, message: String) {
        self.messages.push(format!("{}: {}", sender, message));
    }
}

struct MockApp {
    instances: Vec<MockInstance>,
    max_instances: usize,
    current_tab: usize,
}

impl MockApp {
    fn new() -> Self {
        Self {
            instances: vec![MockInstance::new("Claude 1".to_string())],
            max_instances: 5,
            current_tab: 0,
        }
    }
    
    // Simulate the spawn_coordinated_instances_with_count logic
    fn spawn_coordinated_instances_with_count(&mut self, _main_instance_id: Uuid, breakdown: &str, working_dir: &str, requested_count: usize) -> Vec<MockInstance> {
        let subtasks: Vec<&str> = breakdown.lines()
            .filter(|line| line.starts_with("SUBTASK_"))
            .collect();
        
        let mut spawned_instances = Vec::new();
        
        if subtasks.is_empty() {
            // Handle generic instances for failed breakdown
            if requested_count > 0 {
                for i in 0..requested_count.min(self.max_instances - self.instances.len()) {
                    let instance_name = format!("Claude {}-{}", self.instances.len() + 1, char::from(b'A' + i as u8));
                    let mut new_instance = MockInstance::new(instance_name);
                    new_instance.working_directory = working_dir.to_string();
                    
                    let generic_task = format!("Work on: {}", 
                        if breakdown.starts_with("ERROR:") {
                            "General development tasks (task analysis failed)"
                        } else {
                            breakdown
                        }
                    );
                    
                    new_instance.add_message("System".to_string(), format!("Generic task: {}", generic_task));
                    spawned_instances.push(new_instance.clone());
                    self.instances.push(new_instance);
                }
            }
            return spawned_instances;
        }
        
        // Determine how many instances to spawn
        let instances_to_spawn = if requested_count > 0 {
            requested_count.min(self.max_instances - self.instances.len())
        } else {
            subtasks.len().min(self.max_instances - self.instances.len())
        };
        
        // Spawn instances for each subtask
        for i in 0..instances_to_spawn {
            if self.instances.len() >= self.max_instances {
                break;
            }
            
            let subtask = subtasks.get(i % subtasks.len()).unwrap_or(&"General coordination task");
            
            let instance_name = format!("Claude {}-{}", self.instances.len() + 1, char::from(b'A' + i as u8));
            let mut new_instance = MockInstance::new(instance_name);
            new_instance.working_directory = working_dir.to_string();
            
            // Parse subtask details
            let task_parts: Vec<&str> = subtask.split(" | ").collect();
            let task_desc = task_parts.get(0)
                .unwrap_or(&"")
                .trim_start_matches("SUBTASK_")
                .trim_start_matches("1: ")
                .trim_start_matches("2: ")
                .trim_start_matches("3: ");
            
            let scope = task_parts.iter()
                .find(|part| part.starts_with("SCOPE:"))
                .map(|s| s.trim_start_matches("SCOPE:").trim())
                .unwrap_or("No specific scope");
                
            let priority = task_parts.iter()
                .find(|part| part.starts_with("PRIORITY:"))
                .map(|s| s.trim_start_matches("PRIORITY:").trim())
                .unwrap_or("Medium");
            
            let coordination_message = format!(
                "COORDINATION: Task: {} | Scope: {} | Priority: {} | Dir: {}",
                task_desc, scope, priority, working_dir
            );
            
            new_instance.add_message("System".to_string(), coordination_message);
            spawned_instances.push(new_instance.clone());
            self.instances.push(new_instance);
        }
        
        // Switch to the first new instance
        if instances_to_spawn > 0 {
            let first_new_tab = self.instances.len() - instances_to_spawn;
            if first_new_tab < self.instances.len() {
                self.current_tab = first_new_tab;
            }
        }
        
        spawned_instances
    }
}

#[tokio::test]
async fn test_spawn_instances_message_flow() {
    // Test the complete message flow for spawning instances
    let (tx, mut rx) = mpsc::channel::<MockClaudeMessage>(100);
    
    // Simulate the spawn instances request
    let instance_id = Uuid::new_v4();
    let task_description = "Implement parallel features: Raft consensus, erasure coding, CLI tools".to_string();
    let num_instances = 3;
    
    let spawn_message = MockClaudeMessage::VedaSpawnInstances {
        instance_id,
        task_description: task_description.clone(),
        num_instances,
    };
    
    // Send the message
    tx.send(spawn_message).await.unwrap();
    
    // Simulate receiving and processing the message
    if let Some(msg) = rx.recv().await {
        match msg {
            MockClaudeMessage::VedaSpawnInstances { instance_id: id, task_description: desc, num_instances: count } => {
                assert_eq!(id, instance_id);
                assert_eq!(desc, task_description);
                assert_eq!(count, num_instances);
                println!("✅ VedaSpawnInstances message correctly sent and received");
            }
            _ => panic!("Expected VedaSpawnInstances message"),
        }
    }
}

#[tokio::test]
async fn test_background_coordination_message() {
    // Test the InternalCoordinateInstances message flow
    let (tx, mut rx) = mpsc::channel::<MockClaudeMessage>(100);
    
    let main_instance_id = Uuid::new_v4();
    let task_description = "SUBTASK_1: Implement Raft | SCOPE: master module | PRIORITY: High".to_string();
    let num_instances = 2;
    let working_dir = "/test/project".to_string();
    
    let coord_message = MockClaudeMessage::InternalCoordinateInstances {
        main_instance_id,
        task_description: task_description.clone(),
        num_instances,
        working_dir: working_dir.clone(),
        is_ipc: true,
    };
    
    tx.send(coord_message).await.unwrap();
    
    if let Some(msg) = rx.recv().await {
        match msg {
            MockClaudeMessage::InternalCoordinateInstances { 
                main_instance_id: id, 
                task_description: desc, 
                num_instances: count,
                working_dir: dir,
                is_ipc 
            } => {
                assert_eq!(id, main_instance_id);
                assert_eq!(desc, task_description);
                assert_eq!(count, num_instances);
                assert_eq!(dir, working_dir);
                assert!(is_ipc);
                println!("✅ InternalCoordinateInstances message correctly processed");
            }
            _ => panic!("Expected InternalCoordinateInstances message"),
        }
    }
}

#[tokio::test]
async fn test_instance_spawning_with_valid_breakdown() {
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    // Test with valid DeepSeek breakdown
    let valid_breakdown = r#"SUBTASK_1: Implement Raft consensus | SCOPE: mooseng-master/ | PRIORITY: High
SUBTASK_2: Develop Reed-Solomon coding | SCOPE: mooseng-chunkserver/ | PRIORITY: High  
SUBTASK_3: Create CLI management tools | SCOPE: mooseng-cli/ | PRIORITY: Medium"#;
    
    let working_dir = "/Users/test/moosefs";
    let requested_count = 3;
    
    let initial_count = app.instances.len();
    let spawned = app.spawn_coordinated_instances_with_count(main_instance_id, valid_breakdown, working_dir, requested_count);
    
    assert_eq!(spawned.len(), 3, "Should spawn 3 instances");
    assert_eq!(app.instances.len(), initial_count + 3, "Should have 3 additional instances");
    
    // Check instance names
    assert_eq!(spawned[0].name, "Claude 2-A");
    assert_eq!(spawned[1].name, "Claude 3-B");
    assert_eq!(spawned[2].name, "Claude 4-C");
    
    // Check working directories
    for instance in &spawned {
        assert_eq!(instance.working_directory, working_dir);
    }
    
    // Check coordination messages contain task details
    assert!(spawned[0].messages[0].contains("Implement Raft consensus"));
    assert!(spawned[1].messages[0].contains("Develop Reed-Solomon coding"));
    assert!(spawned[2].messages[0].contains("Create CLI management tools"));
    
    println!("✅ Instance spawning with valid breakdown works correctly");
}

#[tokio::test]
async fn test_instance_spawning_with_failed_breakdown() {
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    // Test with failed breakdown (no SUBTASK_ lines)
    let failed_breakdown = "ERROR: Failed to analyze task - spawning generic instances";
    let working_dir = "/Users/test/project";
    let requested_count = 2;
    
    let initial_count = app.instances.len();
    let spawned = app.spawn_coordinated_instances_with_count(main_instance_id, failed_breakdown, working_dir, requested_count);
    
    assert_eq!(spawned.len(), 2, "Should spawn 2 generic instances");
    assert_eq!(app.instances.len(), initial_count + 2, "Should have 2 additional instances");
    
    // Check that generic instances were created
    assert!(spawned[0].messages[0].contains("General development tasks"));
    assert!(spawned[1].messages[0].contains("General development tasks"));
    
    println!("✅ Instance spawning with failed breakdown creates generic instances");
}

#[tokio::test]
async fn test_tab_switching_logic() {
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    assert_eq!(app.current_tab, 0, "Should start on tab 0");
    
    let breakdown = "SUBTASK_1: Task A | SCOPE: module_a/ | PRIORITY: High\nSUBTASK_2: Task B | SCOPE: module_b/ | PRIORITY: Medium";
    let spawned = app.spawn_coordinated_instances_with_count(main_instance_id, breakdown, "/test", 2);
    
    assert_eq!(spawned.len(), 2, "Should spawn 2 instances");
    
    // Should switch to the first new instance (not back to main)
    let expected_tab = app.instances.len() - 2; // First of the 2 new instances
    assert_eq!(app.current_tab, expected_tab, "Should switch to first new instance");
    
    println!("✅ Tab switching logic works correctly");
}

#[tokio::test]
async fn test_max_instances_limit() {
    let mut app = MockApp::new();
    app.max_instances = 3; // Set low limit for testing
    
    // Already have 1 instance, so can only spawn 2 more
    let main_instance_id = app.instances[0].id;
    let breakdown = "SUBTASK_1: A | SCOPE: a/ | PRIORITY: High\nSUBTASK_2: B | SCOPE: b/ | PRIORITY: High\nSUBTASK_3: C | SCOPE: c/ | PRIORITY: High\nSUBTASK_4: D | SCOPE: d/ | PRIORITY: High";
    
    let spawned = app.spawn_coordinated_instances_with_count(main_instance_id, breakdown, "/test", 5); // Request more than limit
    
    assert_eq!(spawned.len(), 2, "Should only spawn 2 instances (limited by max_instances)");
    assert_eq!(app.instances.len(), 3, "Should not exceed max_instances");
    
    println!("✅ Max instances limit is properly enforced");
}

#[tokio::test]
async fn test_subtask_parsing() {
    // Test parsing of different subtask formats
    let test_cases = vec![
        ("SUBTASK_1: Basic task | SCOPE: src/ | PRIORITY: High", 
         ("Basic task", "src/", "High")),
        ("SUBTASK_2: Complex task with details | SCOPE: multiple/dirs/ | PRIORITY: Medium",
         ("Complex task with details", "multiple/dirs/", "Medium")),
        ("SUBTASK_3: No scope task | PRIORITY: Low",
         ("No scope task", "No specific scope", "Low")),
        ("SUBTASK_1: No priority task | SCOPE: test/",
         ("No priority task", "test/", "Medium")), // Default priority
    ];
    
    for (subtask_line, (expected_desc, expected_scope, expected_priority)) in test_cases {
        let task_parts: Vec<&str> = subtask_line.split(" | ").collect();
        let task_desc = task_parts.get(0)
            .unwrap_or(&"")
            .trim_start_matches("SUBTASK_")
            .trim_start_matches("1: ")
            .trim_start_matches("2: ")
            .trim_start_matches("3: ");
        
        let scope = task_parts.iter()
            .find(|part| part.starts_with("SCOPE:"))
            .map(|s| s.trim_start_matches("SCOPE:").trim())
            .unwrap_or("No specific scope");
            
        let priority = task_parts.iter()
            .find(|part| part.starts_with("PRIORITY:"))
            .map(|s| s.trim_start_matches("PRIORITY:").trim())
            .unwrap_or("Medium");
        
        assert_eq!(task_desc, expected_desc, "Task description should match");
        assert_eq!(scope, expected_scope, "Scope should match");
        assert_eq!(priority, expected_priority, "Priority should match");
    }
    
    println!("✅ Subtask parsing works correctly for all formats");
}

#[tokio::test]
async fn test_working_directory_propagation() {
    let mut app = MockApp::new();
    let main_instance_id = app.instances[0].id;
    
    let test_working_dirs = vec![
        "/Users/test/moosefs",
        "/home/user/project", 
        "/workspace/rust-app",
        "."
    ];
    
    for working_dir in test_working_dirs {
        let breakdown = "SUBTASK_1: Test task | SCOPE: test/ | PRIORITY: High";
        let spawned = app.spawn_coordinated_instances_with_count(main_instance_id, breakdown, working_dir, 1);
        
        assert_eq!(spawned.len(), 1, "Should spawn 1 instance");
        assert_eq!(spawned[0].working_directory, working_dir, "Working directory should be propagated");
        
        // Clean up for next iteration
        app.instances.pop();
    }
    
    println!("✅ Working directory propagation works correctly");
}

#[tokio::test]
async fn test_ipc_vs_direct_spawning() {
    // Test the difference between IPC spawning (from MCP) vs direct spawning
    let (tx, mut rx) = mpsc::channel::<MockClaudeMessage>(100);
    
    // IPC spawning (is_ipc = true)
    let ipc_message = MockClaudeMessage::InternalCoordinateInstances {
        main_instance_id: Uuid::new_v4(),
        task_description: "IPC spawning test".to_string(),
        num_instances: 2,
        working_dir: "/test".to_string(),
        is_ipc: true,
    };
    
    tx.send(ipc_message).await.unwrap();
    
    // Direct spawning (is_ipc = false)  
    let direct_message = MockClaudeMessage::InternalCoordinateInstances {
        main_instance_id: Uuid::new_v4(),
        task_description: "Direct spawning test".to_string(),
        num_instances: 1,
        working_dir: "/test".to_string(),
        is_ipc: false,
    };
    
    tx.send(direct_message).await.unwrap();
    
    // Verify both messages are handled correctly
    let msg1 = rx.recv().await.unwrap();
    let msg2 = rx.recv().await.unwrap();
    
    match (msg1, msg2) {
        (MockClaudeMessage::InternalCoordinateInstances { is_ipc: true, .. },
         MockClaudeMessage::InternalCoordinateInstances { is_ipc: false, .. }) => {
            println!("✅ IPC vs direct spawning messages handled correctly");
        }
        _ => panic!("Expected InternalCoordinateInstances messages with different is_ipc values"),
    }
}