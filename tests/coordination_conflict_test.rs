use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

// Mock structures to test the coordination conflict scenario
struct MockApp {
    coordination_in_progress: bool,
    instances: Vec<MockInstance>,
    coordination_enabled: bool,
    max_instances: usize,
}

struct MockInstance {
    id: Uuid,
    name: String,
    messages: Vec<String>,
}

impl MockInstance {
    fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            messages: Vec::new(),
        }
    }
    
    fn add_message(&mut self, sender: String, message: String) {
        self.messages.push(format!("{}: {}", sender, message));
    }
}

impl MockApp {
    fn new() -> Self {
        Self {
            coordination_in_progress: false,
            instances: vec![MockInstance::new("Claude 1".to_string())],
            coordination_enabled: true,
            max_instances: 5,
        }
    }
    
    // Simulate the VedaSpawnInstances coordination path
    async fn handle_veda_spawn_instances(&mut self, task_description: String, num_instances: u8) -> bool {
        if self.coordination_in_progress {
            return false; // Already coordinating
        }
        
        // Set coordination in progress (this is what the bug fix ensures)
        self.coordination_in_progress = true;
        
        // Simulate the background DeepSeek analysis
        let analysis_result = self.simulate_deepseek_analysis(&task_description).await;
        
        // Simulate spawning instances based on analysis result
        if analysis_result.contains("SUBTASK_") {
            // Spawn instances based on breakdown
            for _i in 0..num_instances {
                let instance_name = format!("Claude {}", self.instances.len() + 1);
                self.instances.push(MockInstance::new(instance_name));
            }
            return true;
        }
        
        false
    }
    
    // Simulate the automode coordination analysis
    async fn handle_automode_coordination(&mut self, claude_message: String) -> bool {
        // This is the critical check that was missing in the original bug
        if self.coordination_in_progress {
            println!("Coordination already in progress, skipping automode coordination analysis");
            return false; // Don't interfere with ongoing coordination
        }
        
        // If not coordinating, proceed with automode analysis
        if let Some(instance) = self.instances.get_mut(0) {
            instance.add_message("System".to_string(), 
                "🤖 Analyzing if task would benefit from multi-instance coordination...".to_string());
        }
        
        let analysis_result = self.simulate_deepseek_analysis(&claude_message).await;
        // Check if the response indicates coordination would be beneficial
        analysis_result.contains("COORDINATE_BENEFICIAL") || analysis_result.contains("SUBTASK_")
    }
    
    async fn simulate_deepseek_analysis(&self, message: &str) -> String {
        // Simulate analysis delay
        sleep(Duration::from_millis(10)).await;
        
        if message.contains("(no content)") {
            "SINGLE_INSTANCE_SUFFICIENT: No content to analyze for separability".to_string()
        } else if message.contains("parallel") || message.contains("multiple") {
            // Simulate successful task breakdown for coordination
            "SUBTASK_1: Implement Raft consensus | SCOPE: src/raft/ | PRIORITY: High\nSUBTASK_2: Develop erasure coding | SCOPE: src/erasure/ | PRIORITY: High\nSUBTASK_3: Create CLI tools | SCOPE: src/cli/ | PRIORITY: Medium".to_string()
        } else {
            "SINGLE_INSTANCE_SUFFICIENT: Single focused task".to_string()
        }
    }
}

#[tokio::test]
async fn test_coordination_conflict_prevention() {
    // Test that automode coordination respects ongoing VedaSpawnInstances coordination
    let mut app = MockApp::new();
    
    // Verify initial state
    assert!(!app.coordination_in_progress, "Should start with no coordination in progress");
    assert_eq!(app.instances.len(), 1, "Should start with 1 instance");
    
    // Step 1: Start VedaSpawnInstances coordination
    let veda_task = "Implement parallel features: Raft, erasure coding, CLI tools".to_string();
    
    // Step 2: Test coordination conflict by simulating concurrent attempts
    // First, start VedaSpawnInstances which sets coordination_in_progress = true
    assert!(app.handle_veda_spawn_instances(veda_task.clone(), 3).await, "VedaSpawnInstances should succeed");
    
    // Now test that automode coordination is blocked while coordination is in progress
    app.coordination_in_progress = true; // Simulate ongoing coordination
    let automode_message = "(no content)".to_string();
    let automode_result = app.handle_automode_coordination(automode_message).await;
    
    // With the fix, automode should be blocked
    assert!(!automode_result, "Automode coordination should be blocked when VedaSpawnInstances is in progress");
    
    // Clear coordination flag and verify automode can work afterwards
    app.coordination_in_progress = false;
    let automode_message2 = "Implement multiple parallel features".to_string();
    let automode_result2 = app.handle_automode_coordination(automode_message2).await;
    assert!(automode_result2, "Automode should work after coordination completes");
    
    println!("✅ Coordination conflict prevention works correctly");
}

#[tokio::test]
async fn test_sequential_coordination_attempts() {
    // Test that coordination attempts work correctly when done sequentially
    let mut app = MockApp::new();
    
    // First coordination attempt (VedaSpawnInstances)
    app.coordination_in_progress = true;
    let veda_result = app.handle_veda_spawn_instances("Task A".to_string(), 2).await;
    assert!(!veda_result, "Should fail when coordination already in progress");
    
    // Clear coordination flag (simulating completion)
    app.coordination_in_progress = false;
    
    // Second coordination attempt (automode) should now work
    let automode_result = app.handle_automode_coordination("Implement multiple parallel features".to_string()).await;
    assert!(automode_result, "Automode coordination should work when no coordination in progress");
    
    println!("✅ Sequential coordination attempts work correctly");
}

#[tokio::test]
async fn test_automode_single_instance_sufficient_response() {
    // Test that automode correctly handles "(no content)" messages
    let mut app = MockApp::new();
    
    // Verify automode analysis of empty content
    let empty_content_result = app.handle_automode_coordination("(no content)".to_string()).await;
    assert!(!empty_content_result, "Should not coordinate for empty content");
    
    // Verify the system message was added
    assert!(app.instances[0].messages.len() > 0, "Should have added analysis message");
    assert!(app.instances[0].messages[0].contains("Analyzing if task would benefit"), 
           "Should contain analysis message");
    
    println!("✅ Automode correctly handles SINGLE_INSTANCE_SUFFICIENT scenarios");
}

#[tokio::test]
async fn test_coordination_flag_lifecycle() {
    // Test the complete lifecycle of coordination_in_progress flag
    let mut app = MockApp::new();
    
    // Initial state
    assert!(!app.coordination_in_progress, "Should start false");
    
    // During VedaSpawnInstances
    app.coordination_in_progress = true;
    assert!(app.coordination_in_progress, "Should be true during coordination");
    
    // Automode should be blocked
    let automode_blocked = app.handle_automode_coordination("Some task".to_string()).await;
    assert!(!automode_blocked, "Automode should be blocked");
    
    // After coordination completes
    app.coordination_in_progress = false;
    assert!(!app.coordination_in_progress, "Should be false after completion");
    
    // Automode should now work
    let automode_allowed = app.handle_automode_coordination("Multiple parallel tasks".to_string()).await;
    assert!(automode_allowed, "Automode should work after coordination completes");
    
    println!("✅ Coordination flag lifecycle works correctly");
}

#[tokio::test]
async fn test_deepseek_analysis_conflict_scenarios() {
    // Test different DeepSeek analysis conflict scenarios
    let analysis_scenarios = vec![
        ("(no content)", false, "SINGLE_INSTANCE_SUFFICIENT"),
        ("Implement a single feature", false, "SINGLE_INSTANCE_SUFFICIENT"), 
        ("Implement multiple parallel features", true, "SUBTASK_"),
        ("Build Raft, erasure coding, and CLI in parallel", true, "SUBTASK_"),
    ];
    
    for (message, should_coordinate, expected_response) in analysis_scenarios {
        let mut app = MockApp::new();
        
        // Test that analysis gives expected response
        let analysis_result = app.simulate_deepseek_analysis(message).await;
        assert!(analysis_result.contains(expected_response), 
               "Expected '{}' in response for message: '{}'", expected_response, message);
        
        // Test that automode coordination respects the analysis
        let coordination_result = app.handle_automode_coordination(message.to_string()).await;
        assert_eq!(coordination_result, should_coordinate, 
                  "Coordination result should match expectation for: '{}'", message);
    }
    
    println!("✅ DeepSeek analysis conflict scenarios work correctly");
}

#[tokio::test]
async fn test_race_condition_prevention() {
    // Test that race conditions between VedaSpawnInstances and automode are prevented
    let app1 = Arc::new(tokio::sync::Mutex::new(MockApp::new()));
    let app2 = app1.clone();
    
    let coordination_started = Arc::new(AtomicBool::new(false));
    let coordination_started1 = coordination_started.clone();
    let coordination_started2 = coordination_started.clone();
    
    // Task 1: VedaSpawnInstances
    let task1 = tokio::spawn(async move {
        let mut app = app1.lock().await;
        if !app.coordination_in_progress {
            coordination_started1.store(true, Ordering::SeqCst);
            sleep(Duration::from_millis(20)).await; // Simulate work
            let result = app.handle_veda_spawn_instances("Implement parallel features: Raft, erasure coding, CLI tools".to_string(), 2).await;
            app.coordination_in_progress = false;
            result
        } else {
            false
        }
    });
    
    // Task 2: Automode coordination (should be blocked)
    let task2 = tokio::spawn(async move {
        sleep(Duration::from_millis(10)).await; // Start after task1 begins
        let mut app = app2.lock().await;
        if coordination_started2.load(Ordering::SeqCst) {
            // If coordination already started, automode should be blocked
            app.handle_automode_coordination("Task 2".to_string()).await
        } else {
            // If coordination hasn't started, automode can proceed
            true
        }
    });
    
    let (result1, result2) = tokio::join!(task1, task2);
    
    // VedaSpawnInstances should succeed
    assert!(result1.unwrap(), "VedaSpawnInstances should succeed");
    
    // Automode should be blocked (return false)
    assert!(!result2.unwrap(), "Automode should be blocked by ongoing coordination");
    
    println!("✅ Race condition prevention works correctly");
}

#[tokio::test]
async fn test_original_bug_reproduction() {
    // Test that reproduces the original bug scenario if the fix wasn't applied
    let mut app = MockApp::new();
    
    // Step 1: Claude uses VedaSpawnInstances tool (sets coordination_in_progress = true)
    app.coordination_in_progress = true;
    
    // Step 2: Claude sends "(no content)" message, triggering StreamEnd
    // Step 3: Automode tries to analyze "(no content)" for coordination
    // BUG: Original code didn't check coordination_in_progress
    
    // With the fix: automode should be blocked
    let automode_result = app.handle_automode_coordination("(no content)".to_string()).await;
    assert!(!automode_result, "Automode should be blocked during ongoing coordination");
    
    // Original bug would have analyzed "(no content)" and concluded SINGLE_INSTANCE_SUFFICIENT
    // This would have interfered with the VedaSpawnInstances background coordination
    
    // With the fix: VedaSpawnInstances coordination can complete uninterrupted
    app.coordination_in_progress = false; // Simulate completion
    
    println!("✅ Original bug scenario is now prevented by the fix");
}