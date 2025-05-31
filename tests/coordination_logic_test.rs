use uuid::Uuid;

// Mock structures to test coordination logic
struct MockApp {
    coordination_enabled: bool,
    instances: Vec<MockInstance>,
    max_instances: usize,
}

struct MockInstance {
    id: Uuid,
    name: String,
}

impl MockApp {
    fn new() -> Self {
        Self {
            coordination_enabled: true,
            instances: vec![MockInstance {
                id: Uuid::new_v4(),
                name: "Claude 1".to_string(),
            }],
            max_instances: 5,
        }
    }


    fn should_coordinate_basic_checks(&self) -> bool {
        self.coordination_enabled && self.instances.len() < self.max_instances
    }
}







#[tokio::test]
async fn test_deepseek_prompt_format() {
    // Test that the DeepSeek prompt is correctly formatted for the fixed logic
    let claude_message = "Implement multiple independent features: Raft consensus, erasure coding, and CLI tools in separate modules.";
    
    let analysis_prompt = format!(
        r#"Analyze if this Claude message indicates a task that would benefit from multiple parallel Claude Code instances working together:

Claude's message: "{}"

Consider these factors for PARALLEL INSTANCES (respond COORDINATE_BENEFICIAL if ANY apply):
1. Multiple independent components/modules that can be worked on separately
2. Multiple separate features that can be developed in parallel  
3. Tasks like "implement X, Y, and Z" where X, Y, Z are separable and independent
4. Testing multiple components simultaneously without interference
5. Documentation generation across multiple independent areas
6. Refactoring that can be divided by file/module boundaries
7. Claude mentions working on multiple files/directories
8. Task involves parallel development streams

IMPORTANT: Independent, separable tasks are IDEAL for parallel instances!

Respond with EXACTLY one of:
COORDINATE_BENEFICIAL: [Brief reason - focus on independence and separability]
SINGLE_INSTANCE_SUFFICIENT: [Brief reason - only if tasks are tightly coupled/interdependent]

Your response:"#,
        claude_message
    );
    
    // Verify the prompt contains the corrected logic
    assert!(analysis_prompt.contains("Independent, separable tasks are IDEAL for parallel instances!"));
    assert!(analysis_prompt.contains("respond COORDINATE_BENEFICIAL if ANY apply"));
    assert!(analysis_prompt.contains("only if tasks are tightly coupled/interdependent"));
    
    println!("✅ DeepSeek prompt correctly formatted with fixed logic");
}

#[tokio::test]
async fn test_coordination_basic_checks() {
    // Test coordination enabled/disabled
    let mut app = MockApp::new();
    assert!(app.should_coordinate_basic_checks(), "Should coordinate when enabled and under max instances");
    
    app.coordination_enabled = false;
    assert!(!app.should_coordinate_basic_checks(), "Should not coordinate when disabled");
    
    // Test max instances limit
    app.coordination_enabled = true;
    app.instances = vec![
        MockInstance { id: Uuid::new_v4(), name: "Claude 1".to_string() },
        MockInstance { id: Uuid::new_v4(), name: "Claude 2".to_string() },
        MockInstance { id: Uuid::new_v4(), name: "Claude 3".to_string() },
        MockInstance { id: Uuid::new_v4(), name: "Claude 4".to_string() },
        MockInstance { id: Uuid::new_v4(), name: "Claude 5".to_string() },
    ];
    assert!(!app.should_coordinate_basic_checks(), "Should not coordinate when at max instances");
    
    println!("✅ Basic coordination checks work correctly");
}

#[tokio::test]
async fn test_explicit_coordination_keywords() {
    let _app = MockApp::new();
    
    // Test explicit coordination request keywords
    let explicit_requests = [
        "spawn additional instances for this work",
        "I need multiple instances to work in parallel", 
        "please divide and conquer this task",
        "coordinate with other instances on this",
        "split this task across multiple instances",
        "we should work in parallel on different parts"
    ];
    
    for request in &explicit_requests {
        // Check for explicit keywords (this simulates the explicit keyword check)
        let message_lower = request.to_lowercase();
        let explicit_keywords = [
            "spawn additional instances",
            "multiple instances", 
            "parallel processing",
            "divide and conquer",
            "coordinate with other instances",
            "split this task",
            "work in parallel"
        ];
        
        let has_explicit = explicit_keywords.iter().any(|keyword| message_lower.contains(keyword));
        assert!(has_explicit, "Should detect explicit coordination request: {}", request);
    }
    
    println!("✅ Explicit coordination keywords detected correctly");
}

#[tokio::test]
async fn test_spawn_instances_message_format() {
    // Test the message format for spawning instances
    let spawn_request = serde_json::json!({
        "type": "spawn_instances",
        "session_id": "test-session-123",
        "task_description": "Implement parallel features: Raft in master module, erasure coding in chunkserver module, CLI tools in cli module",
        "num_instances": 3
    });
    
    assert_eq!(spawn_request["type"], "spawn_instances");
    assert_eq!(spawn_request["num_instances"], 3);
    assert!(spawn_request["task_description"].as_str().unwrap().contains("parallel"));
    
    println!("✅ Spawn instances message format is correct");
}

#[tokio::test]
async fn test_coordination_message_construction() {
    // Test that coordination messages are properly constructed
    let task_desc = "Implement Raft consensus";
    let scope = "mooseng-master module, src/raft/";
    let priority = "High";
    let working_dir = "/Users/test/moosefs";
    
    let coordination_message = format!(
        r#"You are part of a coordinated team of Claude instances working on a shared codebase.

YOUR ASSIGNED SUBTASK: {}
SCOPE: {}
PRIORITY: {}
WORKING DIRECTORY: {}

COORDINATION PROTOCOL:
1. Use TaskMaster AI tools to stay in sync
2. Focus ONLY on your assigned scope to avoid conflicts
3. Update main instance with major progress
4. Use TaskMaster to communicate completion status

IMPORTANT: Work within your scope and coordinate via TaskMaster!"#,
        task_desc, scope, priority, working_dir
    );
    
    assert!(coordination_message.contains("coordinated team"));
    assert!(coordination_message.contains(task_desc));
    assert!(coordination_message.contains(scope));
    assert!(coordination_message.contains(priority));
    assert!(coordination_message.contains(working_dir));
    assert!(coordination_message.contains("TaskMaster"));
    assert!(coordination_message.contains("avoid conflicts"));
    
    println!("✅ Coordination message properly constructed");
}


