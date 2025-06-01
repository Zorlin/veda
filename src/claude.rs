use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::process::Command as AsyncCommand;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum ClaudeMessage {
    StreamStart { session_id: Option<String> },
    StreamText { text: String, session_id: Option<String> },
    StreamEnd { session_id: Option<String> },
    Error { error: String, session_id: Option<String> },
    Exited { code: Option<i32>, session_id: Option<String> },
    ToolUse { tool_name: String, session_id: Option<String> },
    SessionStarted { session_id: String, target_tab_id: Option<uuid::Uuid> },
    ToolPermissionDenied { tool_name: String, session_id: Option<String> },
    // Instance management MCP calls
    VedaSpawnInstances { instance_id: Uuid, task_description: String, num_instances: u8 },
    VedaListInstances { instance_id: Uuid },
    VedaCloseInstance { instance_id: Uuid, target_instance_name: String },
    // Internal message for background coordination
    InternalCoordinateInstances { 
        main_instance_id: Uuid, 
        task_description: String, 
        num_instances: usize, 
        working_dir: String,
        is_ipc: bool,
    },
    // Inter-Veda coordination message
    CoordinationMessage { 
        message: crate::shared_ipc::VedaCoordinationMessage,
    },
    // Process handle update for tool auto-approval
    ProcessHandleUpdate {
        session_id: Option<String>,
        process_handle: Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
    },
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ClaudeStreamEvent {
    #[serde(rename = "system")]
    System {
        subtype: String,
        session_id: String,
    },
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
        session_id: String,
    },
    #[serde(rename = "user")]
    User {
        message: serde_json::Value,
        session_id: String,
    },
    #[serde(rename = "result")]
    Result {
        subtype: String,
        result: Option<String>,
        is_error: bool,
        session_id: String,
    },
    #[serde(rename = "error")]
    Error { error: ErrorInfo },
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct AssistantMessage {
    pub id: String,
    pub content: Vec<ContentItem>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentItem {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}


#[derive(Debug, Deserialize, PartialEq)]
pub struct ErrorInfo {
    pub message: String,
}

/// Check if a tool is already enabled in Claude's configuration
pub async fn is_tool_enabled(tool_name: &str) -> Result<bool> {
    let cmd = AsyncCommand::new("claude")
        .arg("config")
        .arg("get")
        .arg("allowedTools")
        .output()
        .await?;
    
    if !cmd.status.success() {
        tracing::warn!("Failed to check if tool {} is enabled", tool_name);
        return Ok(false);
    }
    
    let output = String::from_utf8_lossy(&cmd.stdout);
    let is_enabled = output.contains(tool_name);
    tracing::info!("Tool '{}' enabled status: {}", tool_name, is_enabled);
    Ok(is_enabled)
}

/// Enable a tool in Claude's configuration
pub async fn enable_claude_tool(tool_name: &str) -> Result<()> {
    tracing::info!("Enabling Claude tool: {}", tool_name);
    
    // Check if already enabled first
    if let Ok(true) = is_tool_enabled(tool_name).await {
        tracing::info!("Tool '{}' is already enabled, skipping", tool_name);
        return Ok(());
    }
    
    let cmd = AsyncCommand::new("claude")
        .arg("config")
        .arg("add")
        .arg("allowedTools")
        .arg(tool_name)
        .output()
        .await?;
    
    if !cmd.status.success() {
        let error_output = String::from_utf8_lossy(&cmd.stderr);
        tracing::error!("Failed to enable tool {}: {}", tool_name, error_output);
        return Err(anyhow::anyhow!("Failed to enable tool: {}", error_output));
    }
    
    let output = String::from_utf8_lossy(&cmd.stdout);
    tracing::info!("Successfully enabled tool {}: {}", tool_name, output);
    Ok(())
}

pub async fn send_to_claude(
    message: String,
    tx: mpsc::Sender<ClaudeMessage>,
) -> Result<()> {
    send_to_claude_with_session(message, tx, None, None, None).await
}

impl ClaudeStreamEvent {
    /// Extract tool name from permission denied messages
    fn extract_permission_denied_tool(message: &serde_json::Value) -> Option<String> {
        // Look for the structure: message.content[0].content contains permission text
        if let Some(content_array) = message.get("content").and_then(|c| c.as_array()) {
            for content_item in content_array {
                if let Some(content_text) = content_item.get("content").and_then(|c| c.as_str()) {
                    if let Some(is_error) = content_item.get("is_error").and_then(|e| e.as_bool()) {
                        if is_error && content_text.contains("but you haven't granted it yet") {
                            // Extract tool name from the standardized message format
                            if let Some(start) = content_text.find("Claude requested permissions to use ") {
                                let tool_part = &content_text[start + "Claude requested permissions to use ".len()..];
                                if let Some(end) = tool_part.find(", but you haven't granted it yet") {
                                    return Some(tool_part[..end].trim().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

pub async fn send_to_claude_with_session(
    message: String,
    tx: mpsc::Sender<ClaudeMessage>,
    session_id: Option<String>,
    process_handle_storage: Option<Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>>,
    target_tab_id: Option<uuid::Uuid>,
) -> Result<()> {
    tracing::info!("send_to_claude_with_session called with message: {} (session: {:?})", message, session_id);
    
    // Build command args based on whether we have a session ID
    let mut cmd = AsyncCommand::new("claude");
    
    // Set the VEDA_SESSION_ID environment variable if available
    if let Ok(veda_session_id) = std::env::var("VEDA_SESSION_ID") {
        cmd.env("VEDA_SESSION_ID", veda_session_id);
    }
    
    // For new conversations, start without session ID to get one from Claude
    // For resuming, use the provided session ID
    // We eliminate instance_id from the flow entirely
    
    let session_id_for_log = session_id.clone();
    if let Some(session) = session_id {
        cmd.arg("--resume").arg(session);
    }
    
    cmd.arg("-p")
        .arg(&message)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--mcp-config")
        .arg(".mcp.json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        
    let mut cmd = cmd.spawn()
        .map_err(|e| {
            tracing::error!("Failed to spawn claude process: {}", e);
            e
        })?;

    let process_pid = cmd.id();
    tracing::info!("Claude process spawned successfully with PID: {:?} (session: {:?})", 
        process_pid, session_id_for_log);
    
    // Extract stdout and stderr first, before storing the process handle
    let stdout = cmd.stdout.take().expect("Failed to open stdout");
    let stderr = cmd.stderr.take().expect("Failed to open stderr");
    
    // Store the process handle if storage was provided
    if let Some(ref handle_storage) = process_handle_storage {
        let mut handle_guard = handle_storage.lock().await;
        *handle_guard = Some(cmd);
        tracing::info!("Stored process handle (PID: {:?})", process_pid);
    }

    // Notify start - use session_id for routing
    tx.send(ClaudeMessage::StreamStart { session_id: session_id_for_log.clone() }).await?;
    tracing::debug!("Sent StreamStart message for session {:?}", session_id_for_log);

    // Spawn task to read stdout
    let tx_stdout = tx.clone();
    let session_id_clone = session_id_for_log.clone();
    tokio::spawn(async move {
        tracing::debug!("Starting stdout reader task for session {:?}", session_id_clone);
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut line_count = 0;
        
        while let Ok(Some(line)) = lines.next_line().await {
            line_count += 1;
            tracing::debug!("STDOUT line {}: {}", line_count, line);
            
            // Parse JSON streaming events
            match serde_json::from_str::<ClaudeStreamEvent>(&line) {
                Ok(event) => {
                    tracing::debug!("Parsed event: {:?}", event);
                    match event {
                        ClaudeStreamEvent::System { subtype, session_id } => {
                            if subtype == "init" {
                                tracing::info!("Session started with ID: {}", session_id);
                                
                                // Simple session started message - shared registry handles coordination
                                let _ = tx_stdout.send(ClaudeMessage::SessionStarted {
                                    session_id,
                                    target_tab_id,
                                }).await;
                            }
                        }
                        ClaudeStreamEvent::Assistant { message, session_id } => {
                            // Extract text and tool uses from the assistant message
                            for content in message.content {
                                match content {
                                    ContentItem::Text { text } => {
                                        tracing::info!("Received assistant text for session {:?}: {:?}", session_id, text);
                                        let _ = tx_stdout.send(ClaudeMessage::StreamText {
                                            text,
                                            session_id: Some(session_id.clone()),
                                        }).await;
                                    }
                                    ContentItem::ToolUse { name, input, .. } => {
                                        tracing::info!("Claude attempting to use tool: {}", name);
                                        
                                        // Check for Veda instance management MCP calls
                                        // Use a placeholder instance_id since tools still need it for socket communication
                                        let placeholder_instance_id = Uuid::new_v4();
                                        match name.as_str() {
                                            "veda_spawn_instances" => {
                                                let task_description = input.as_object()
                                                    .and_then(|obj| obj.get("task_description"))
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let num_instances = input.as_object()
                                                    .and_then(|obj| obj.get("num_instances"))
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(2) as u8; // Default to 2 additional instances
                                                
                                                let _ = tx_stdout.send(ClaudeMessage::VedaSpawnInstances {
                                                    instance_id: placeholder_instance_id,
                                                    task_description,
                                                    num_instances,
                                                }).await;
                                            }
                                            "veda_list_instances" => {
                                                let _ = tx_stdout.send(ClaudeMessage::VedaListInstances {
                                                    instance_id: placeholder_instance_id,
                                                }).await;
                                            }
                                            "veda_close_instance" => {
                                                let target_instance_name = input.as_object()
                                                    .and_then(|obj| obj.get("instance_name"))
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                
                                                let _ = tx_stdout.send(ClaudeMessage::VedaCloseInstance {
                                                    instance_id: placeholder_instance_id,
                                                    target_instance_name,
                                                }).await;
                                            }
                                            _ => {
                                                // Regular tool use
                                                let _ = tx_stdout.send(ClaudeMessage::ToolUse {
                                                    tool_name: name,
                                                    session_id: Some(session_id.clone()),
                                                }).await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ClaudeStreamEvent::Result { result, is_error, session_id, .. } => {
                            if !is_error {
                                tracing::info!("Received result event, ending stream");
                                let _ = tx_stdout.send(ClaudeMessage::StreamEnd {
                                    session_id: Some(session_id),
                                }).await;
                            } else if let Some(error_msg) = result {
                                tracing::error!("Error in result: {}", error_msg);
                                let _ = tx_stdout.send(ClaudeMessage::Error {
                                    error: error_msg,
                                    session_id: Some(session_id),
                                }).await;
                            }
                        }
                        ClaudeStreamEvent::Error { error } => {
                            tracing::error!("Received error from Claude: {}", error.message);
                            let _ = tx_stdout.send(ClaudeMessage::Error {
                                error: error.message,
                                session_id: session_id_clone.clone(), // Use this Claude process's session ID
                            }).await;
                        }
                        ClaudeStreamEvent::User { message, session_id } => {
                            // Check if this is a tool permission denied message
                            if let Some(tool_name) = ClaudeStreamEvent::extract_permission_denied_tool(&message) {
                                tracing::info!("Tool permission denied for: {}", tool_name);
                                let _ = tx_stdout.send(ClaudeMessage::ToolPermissionDenied {
                                    tool_name,
                                    session_id: Some(session_id),
                                }).await;
                            } else {
                                tracing::debug!("Ignoring User event (no permission issue detected)");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse JSON line: {} - Error: {}", line, e);
                }
            }
        }
        tracing::info!("Stdout reader task finished after {} lines", line_count);
    });

    // Spawn task to read stderr
    let tx_stderr = tx.clone();
    let session_id_stderr = session_id_for_log.clone();
    tokio::spawn(async move {
        tracing::debug!("Starting stderr reader task for session {:?}", session_id_stderr);
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut line_count = 0;
        
        while let Ok(Some(line)) = lines.next_line().await {
            line_count += 1;
            tracing::debug!("STDERR line {}: {}", line_count, line);
            
            // Log all stderr output for debugging
            if line.contains("error") || line.contains("Error") {
                tracing::error!("Error from Claude stderr: {}", line);
                let _ = tx_stderr.send(ClaudeMessage::Error {
                    error: line,
                    session_id: session_id_stderr.clone(),
                }).await;
            } else {
                // Log verbose output
                tracing::info!("Claude verbose output: {}", line);
            }
        }
        tracing::info!("Stderr reader task finished after {} lines", line_count);
    });

    // Wait for the process to complete
    let session_id_exit = session_id_for_log.clone();
    let handle_storage_clone = process_handle_storage.as_ref().map(|h| h.clone());
    tokio::spawn(async move {
        tracing::debug!("Waiting for claude process to exit for session {:?}", session_id_exit);
        
        let wait_result = if let Some(handle_storage) = handle_storage_clone {
            // Wait using the stored process handle
            let mut handle_guard = handle_storage.lock().await;
            if let Some(ref mut stored_cmd) = handle_guard.as_mut() {
                let result = stored_cmd.wait().await;
                drop(handle_guard);
                result
            } else {
                drop(handle_guard);
                Err(std::io::Error::new(std::io::ErrorKind::Other, "No process handle available"))
            }
        } else {
            // No process handle storage was provided, which means we can't wait for completion
            // This is not necessarily an error - just means SIGINT functionality won't be available
            tracing::debug!("No process handle storage provided for session {:?}, skipping wait", session_id_exit);
            return; // Exit the spawn task early
        };
        
        match wait_result {
            Ok(status) => {
                let exit_code = status.code();
                tracing::info!("Claude process exited with code: {:?}", exit_code);
                let _ = tx.send(ClaudeMessage::Exited {
                    code: exit_code,
                    session_id: session_id_exit.clone(),
                }).await;
            }
            Err(e) => {
                tracing::error!("Error waiting for claude process: {}", e);
                let _ = tx.send(ClaudeMessage::Error {
                    error: e.to_string(),
                    session_id: session_id_exit.clone(),
                }).await;
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_assistant_message() {
        let json = r#"{"type":"assistant","message":{"id":"msg_123","content":[{"type":"text","text":"Hello, world!"}]},"session_id":"sess_123"}"#;
        let event = serde_json::from_str::<ClaudeStreamEvent>(json).unwrap();
        
        match event {
            ClaudeStreamEvent::Assistant { message, .. } => {
                assert_eq!(message.content.len(), 1);
                if let ContentItem::Text { text } = &message.content[0] {
                    assert_eq!(text, "Hello, world!");
                } else {
                    panic!("Expected text content");
                }
            }
            _ => panic!("Expected Assistant event"),
        }
    }

    #[test]
    fn test_parse_result_success() {
        let json = r#"{"type":"result","subtype":"success","result":"Test result","is_error":false,"session_id":"sess_123"}"#;
        let event = serde_json::from_str::<ClaudeStreamEvent>(json).unwrap();
        
        match event {
            ClaudeStreamEvent::Result { result, is_error, .. } => {
                assert_eq!(result, Some("Test result".to_string()));
                assert!(!is_error);
            }
            _ => panic!("Expected Result event"),
        }
    }

    #[test]
    fn test_parse_error() {
        let json = r#"{"type":"error","error":{"message":"API key not found"}}"#;
        let event = serde_json::from_str::<ClaudeStreamEvent>(json).unwrap();
        
        match event {
            ClaudeStreamEvent::Error { error } => {
                assert_eq!(error.message, "API key not found");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[tokio::test]
    async fn test_claude_message_channel() {
        let (tx, mut rx) = mpsc::channel(10);
        
        // Send different message types
        tx.send(ClaudeMessage::StreamStart { session_id: None }).await.unwrap();
        tx.send(ClaudeMessage::StreamText { 
            text: "Test".to_string(),
            session_id: None,
        }).await.unwrap();
        
        // Receive and verify
        let msg1 = rx.recv().await.unwrap();
        assert!(matches!(msg1, ClaudeMessage::StreamStart { .. }));
        
        let msg2 = rx.recv().await.unwrap();
        match msg2 {
            ClaudeMessage::StreamText { text, .. } => assert_eq!(text, "Test"),
            _ => panic!("Expected StreamText"),
        }
    }
}