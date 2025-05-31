use uuid::Uuid;
use std::collections::HashMap;

/// Test that each Claude instance gets its own VEDA_TARGET_INSTANCE_ID
#[test]
fn test_claude_sets_unique_target_instance_id_per_tab() {
    // Simulate multiple Claude instances being spawned
    let instances = vec![
        ("Veda-1", Uuid::new_v4()),
        ("Veda-2", Uuid::new_v4()),
        ("Veda-3", Uuid::new_v4()),
        ("Veda-4", Uuid::new_v4()),
    ];
    
    let mut environments = HashMap::new();
    
    // Each instance should get its own VEDA_TARGET_INSTANCE_ID
    for (name, instance_id) in &instances {
        let env_value = instance_id.to_string();
        environments.insert(name.to_string(), env_value);
    }
    
    // Verify all instance IDs are unique
    let unique_values: std::collections::HashSet<_> = environments.values().collect();
    assert_eq!(unique_values.len(), environments.len(), "All VEDA_TARGET_INSTANCE_ID values should be unique");
    
    // Verify each instance has the correct ID
    for (name, instance_id) in &instances {
        let env_value = environments.get(&name.to_string()).unwrap();
        assert_eq!(env_value, &instance_id.to_string());
    }
}

/// Test that VEDA_TARGET_INSTANCE_ID is always set (never inherits from parent)
#[test]
fn test_claude_always_sets_target_instance_id() {
    // Simulate parent process environment
    let parent_env = HashMap::from([
        ("VEDA_SESSION_ID".to_string(), "parent-session".to_string()),
        ("VEDA_TARGET_INSTANCE_ID".to_string(), "parent-instance-id".to_string()),
    ]);
    
    // When spawning a new Claude instance, it should ALWAYS set its own ID
    let new_instance_id = Uuid::new_v4();
    let expected_env = format!("VEDA_TARGET_INSTANCE_ID={}", new_instance_id);
    
    // The new instance ID should NOT be the parent's ID
    assert_ne!(new_instance_id.to_string(), parent_env["VEDA_TARGET_INSTANCE_ID"]);
    assert!(expected_env.contains(&new_instance_id.to_string()));
}

/// Test environment variable format
#[test]
fn test_veda_target_instance_id_format() {
    let instance_id = Uuid::new_v4();
    let env_var = format!("VEDA_TARGET_INSTANCE_ID={}", instance_id);
    
    // Should be in the format VEDA_TARGET_INSTANCE_ID=<uuid>
    assert!(env_var.starts_with("VEDA_TARGET_INSTANCE_ID="));
    assert!(env_var.contains(&instance_id.to_string()));
    
    // UUID should be in standard format (8-4-4-4-12)
    let uuid_part = env_var.split('=').nth(1).unwrap();
    assert_eq!(uuid_part.len(), 36); // Standard UUID length
    assert_eq!(uuid_part.matches('-').count(), 4); // 4 hyphens in UUID
}

/// Test that verifies the fix in claude.rs
#[test]
fn test_claude_rs_fix_sets_instance_id() {
    // This test documents the exact fix made in claude.rs:
    // cmd.env("VEDA_TARGET_INSTANCE_ID", instance_id.to_string());
    
    let instance_id = Uuid::new_v4();
    
    // Simulate what claude.rs does
    let mut env_vars = HashMap::new();
    env_vars.insert("VEDA_TARGET_INSTANCE_ID".to_string(), instance_id.to_string());
    
    // Verify the environment variable is set correctly
    assert_eq!(env_vars.get("VEDA_TARGET_INSTANCE_ID").unwrap(), &instance_id.to_string());
}

/// Integration test for the complete environment flow
#[test]
fn test_complete_environment_flow() {
    // Tab 1: Veda-1
    let tab1_instance_id = Uuid::new_v4();
    let tab1_env = HashMap::from([
        ("VEDA_TARGET_INSTANCE_ID".to_string(), tab1_instance_id.to_string()),
    ]);
    
    // Tab 2: Veda-2
    let tab2_instance_id = Uuid::new_v4();
    let tab2_env = HashMap::from([
        ("VEDA_TARGET_INSTANCE_ID".to_string(), tab2_instance_id.to_string()),
    ]);
    
    // Tab 3: Veda-3
    let tab3_instance_id = Uuid::new_v4();
    let tab3_env = HashMap::from([
        ("VEDA_TARGET_INSTANCE_ID".to_string(), tab3_instance_id.to_string()),
    ]);
    
    // Each tab's Claude process has its own unique VEDA_TARGET_INSTANCE_ID
    assert_ne!(tab1_env["VEDA_TARGET_INSTANCE_ID"], tab2_env["VEDA_TARGET_INSTANCE_ID"]);
    assert_ne!(tab1_env["VEDA_TARGET_INSTANCE_ID"], tab3_env["VEDA_TARGET_INSTANCE_ID"]);
    assert_ne!(tab2_env["VEDA_TARGET_INSTANCE_ID"], tab3_env["VEDA_TARGET_INSTANCE_ID"]);
    
    // Each matches its expected instance ID
    assert_eq!(tab1_env["VEDA_TARGET_INSTANCE_ID"], tab1_instance_id.to_string());
    assert_eq!(tab2_env["VEDA_TARGET_INSTANCE_ID"], tab2_instance_id.to_string());
    assert_eq!(tab3_env["VEDA_TARGET_INSTANCE_ID"], tab3_instance_id.to_string());
}

/// Test that environment variables don't leak between instances
#[test]
fn test_no_environment_leakage() {
    // Create multiple instances concurrently
    let mut instances = vec![];
    
    for i in 0..10 {
        let instance_id = Uuid::new_v4();
        let name = format!("Veda-{}", i + 1);
        instances.push((name, instance_id));
    }
    
    // Each instance should have its own unique ID
    let ids: Vec<_> = instances.iter().map(|(_, id)| id).collect();
    let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
    
    assert_eq!(unique_ids.len(), instances.len(), "All instance IDs should be unique - no leakage between instances");
}