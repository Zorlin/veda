use std::fs;
use std::io::{Write, Read};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_debug_log_appends_to_existing_file() {
    // Create a temporary directory for testing
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("debug.log");
    
    // Create initial log file with some content
    let initial_content = "Initial log entry\n";
    fs::write(&log_path, initial_content).unwrap();
    
    // Simulate opening the log file in append mode
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_path)
        .unwrap();
    
    // Write additional content
    writeln!(file, "Appended log entry").unwrap();
    file.flush().unwrap();
    drop(file);
    
    // Read the entire file to verify append worked
    let mut contents = String::new();
    fs::File::open(&log_path).unwrap().read_to_string(&mut contents).unwrap();
    
    // Verify both entries exist
    assert!(contents.contains("Initial log entry"));
    assert!(contents.contains("Appended log entry"));
    assert_eq!(contents.lines().count(), 2);
}

#[test]
fn test_debug_log_creates_file_if_not_exists() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("debug.log");
    
    // Ensure file doesn't exist
    assert!(!log_path.exists());
    
    // Open with create and append
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_path)
        .unwrap();
    
    writeln!(file, "First log entry").unwrap();
    drop(file);
    
    // Verify file was created
    assert!(log_path.exists());
    
    // Verify content
    let contents = fs::read_to_string(&log_path).unwrap();
    assert_eq!(contents.trim(), "First log entry");
}

#[test]
fn test_multiple_append_sessions() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("debug.log");
    
    // First session
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_path)
            .unwrap();
        writeln!(file, "Session 1 - Entry 1").unwrap();
        writeln!(file, "Session 1 - Entry 2").unwrap();
    }
    
    // Second session
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_path)
            .unwrap();
        writeln!(file, "Session 2 - Entry 1").unwrap();
        writeln!(file, "Session 2 - Entry 2").unwrap();
    }
    
    // Third session
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_path)
            .unwrap();
        writeln!(file, "Session 3 - Entry 1").unwrap();
    }
    
    // Verify all entries exist in order
    let contents = fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "Session 1 - Entry 1");
    assert_eq!(lines[1], "Session 1 - Entry 2");
    assert_eq!(lines[2], "Session 2 - Entry 1");
    assert_eq!(lines[3], "Session 2 - Entry 2");
    assert_eq!(lines[4], "Session 3 - Entry 1");
}