use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde::{Deserialize, Serialize};
use serde_json::json;
use anyhow::Result;
use tracing::{info, error, warn};
use uuid;

/// Get the appropriate socket path for the current OS
pub fn get_socket_path() -> String {
    #[cfg(target_os = "macos")]
    {
        // On macOS, use /tmp which is standard and accessible
        "/tmp/veda-shared-registry.sock".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        // On Linux, check if /run/user exists (systemd user session), otherwise use /tmp
        let uid = unsafe { libc::getuid() };
        let user_runtime_dir = format!("/run/user/{}", uid);
        if std::path::Path::new(&user_runtime_dir).exists() {
            format!("{}/veda-shared-registry.sock", user_runtime_dir)
        } else {
            "/tmp/veda-shared-registry.sock".to_string()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // Fallback for other Unix-like systems
        "/tmp/veda-shared-registry.sock".to_string()
    }
}

/// Shared state for tracking instances across multiple Veda processes
#[derive(Debug, Default, Clone)]
pub struct SharedInstanceRegistry {
    /// Map of Session ID -> number of child instances
    session_instances: Arc<RwLock<HashMap<String, u32>>>,
    /// Map of Session ID -> Veda PID for cross-process coordination  
    session_to_pid: Arc<RwLock<HashMap<String, u32>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegistryCommand {
    pub command: String,
    pub session_id: String,
    pub value: Option<u32>,
}

/// Inter-Veda coordination message for cross-codebase communication
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VedaCoordinationMessage {
    pub from: String,           // Source repository/Veda instance name
    pub to: Option<String>,     // Target repository (None = broadcast)
    pub message_type: String,   // "RequestChange", "Acknowledge", "Question", "TaskUpdate", etc.
    pub summary: String,        // Brief description of the request/response
    pub content: String,        // Detailed content
    pub task_id: Option<String>, // TaskMaster AI task ID if applicable
    pub timestamp: u64,         // Unix timestamp
    pub reply_to: Option<String>, // Message ID this is replying to
    pub session_context: Option<String>, // Claude session context if needed
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegistryResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<HashMap<String, u32>>,
}

impl SharedInstanceRegistry {
    pub fn new() -> Self {
        Self {
            session_instances: Arc::new(RwLock::new(HashMap::new())),
            session_to_pid: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn increment_instances(&self, session_id: &str, count: u32) -> u32 {
        let mut registry = self.session_instances.write().await;
        let current = registry.entry(session_id.to_string()).or_insert(0);
        *current += count;
        *current
    }

    pub async fn decrement_instances(&self, session_id: &str, count: u32) -> u32 {
        let mut registry = self.session_instances.write().await;
        if let Some(current) = registry.get_mut(session_id) {
            *current = current.saturating_sub(count);
            if *current == 0 {
                registry.remove(session_id);
                0
            } else {
                *current
            }
        } else {
            0
        }
    }

    pub async fn get_instances(&self, session_id: &str) -> u32 {
        let registry = self.session_instances.read().await;
        registry.get(session_id).copied().unwrap_or(0)
    }

    pub async fn get_all_sessions(&self) -> HashMap<String, u32> {
        let registry = self.session_instances.read().await;
        registry.clone()
    }

    pub async fn clear_session(&self, session_id: &str) {
        let mut registry = self.session_instances.write().await;
        registry.remove(session_id);
    }
    
    /// Register sessionID -> Veda PID mapping
    pub async fn register_session_pid(&self, session_id: &str, veda_pid: u32) {
        let mut registry = self.session_to_pid.write().await;
        registry.insert(session_id.to_string(), veda_pid);
    }
    
    /// Get Veda PID for a session
    pub async fn get_session_pid(&self, session_id: &str) -> Option<u32> {
        let registry = self.session_to_pid.read().await;
        registry.get(session_id).copied()
    }
    
    /// Remove sessionID -> PID mapping
    pub async fn unregister_session_pid(&self, session_id: &str) {
        let mut registry = self.session_to_pid.write().await;
        registry.remove(session_id);
    }
    
    /// Get all sessionID -> PID mappings
    pub async fn get_all_session_pids(&self) -> HashMap<String, u32> {
        let registry = self.session_to_pid.read().await;
        registry.clone()
    }
}

/// Start the shared IPC server that multiple Veda instances can connect to
pub async fn start_shared_ipc_server(app_tx: Option<tokio::sync::mpsc::Sender<crate::claude::ClaudeMessage>>) -> Result<()> {
    let socket_path = get_socket_path();
    
    // Remove existing socket if it exists
    let _ = std::fs::remove_file(&socket_path);
    
    let listener = UnixListener::bind(&socket_path)?;
    info!("Shared IPC registry server listening on {}", socket_path);
    
    let registry = SharedInstanceRegistry::new();
    
    loop {
        match listener.accept().await {
            Ok((socket, _)) => {
                let registry = registry.clone();
                let app_tx_clone = app_tx.clone();
                tokio::spawn(handle_registry_connection(socket, registry, app_tx_clone));
            }
            Err(e) => {
                error!("Failed to accept registry connection: {}", e);
            }
        }
    }
}

async fn handle_registry_connection(
    mut socket: UnixStream,
    registry: SharedInstanceRegistry,
    app_tx: Option<tokio::sync::mpsc::Sender<crate::claude::ClaudeMessage>>,
) {
    let (reader, mut writer) = socket.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    
    while reader.read_line(&mut line).await.is_ok() {
        if line.is_empty() {
            break;
        }
        
        // Try parsing as RegistryCommand first, then as MCP message
        if let Ok(cmd) = serde_json::from_str::<RegistryCommand>(&line) {
                let response = match cmd.command.as_str() {
                    "increment" => {
                        let count = cmd.value.unwrap_or(1);
                        let total = registry.increment_instances(&cmd.session_id, count).await;
                        RegistryResponse {
                            success: true,
                            message: format!("Incremented to {} instances", total),
                            data: None,
                        }
                    }
                    "decrement" => {
                        let count = cmd.value.unwrap_or(1);
                        let remaining = registry.decrement_instances(&cmd.session_id, count).await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} instances remaining", remaining),
                            data: None,
                        }
                    }
                    "get" => {
                        let count = registry.get_instances(&cmd.session_id).await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} instances for session", count),
                            data: Some(HashMap::from([(cmd.session_id.clone(), count)])),
                        }
                    }
                    "list" => {
                        let all_sessions = registry.get_all_sessions().await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} active sessions", all_sessions.len()),
                            data: Some(all_sessions),
                        }
                    }
                    "clear" => {
                        registry.clear_session(&cmd.session_id).await;
                        RegistryResponse {
                            success: true,
                            message: "Session cleared".to_string(),
                            data: None,
                        }
                    }
                    "register_pid" => {
                        let veda_pid = cmd.value.unwrap_or(0);
                        registry.register_session_pid(&cmd.session_id, veda_pid).await;
                        RegistryResponse {
                            success: true,
                            message: format!("Registered session {} -> PID {}", cmd.session_id, veda_pid),
                            data: None,
                        }
                    }
                    "get_pid" => {
                        if let Some(veda_pid) = registry.get_session_pid(&cmd.session_id).await {
                            RegistryResponse {
                                success: true,
                                message: format!("Session {} -> PID {}", cmd.session_id, veda_pid),
                                data: Some(HashMap::from([(cmd.session_id.clone(), veda_pid)])),
                            }
                        } else {
                            RegistryResponse {
                                success: false,
                                message: format!("No PID found for session {}", cmd.session_id),
                                data: None,
                            }
                        }
                    }
                    "unregister_pid" => {
                        registry.unregister_session_pid(&cmd.session_id).await;
                        RegistryResponse {
                            success: true,
                            message: format!("Unregistered session {}", cmd.session_id),
                            data: None,
                        }
                    }
                    "list_pids" => {
                        let all_pids = registry.get_all_session_pids().await;
                        RegistryResponse {
                            success: true,
                            message: format!("{} session PIDs", all_pids.len()),
                            data: Some(all_pids),
                        }
                    }
                    _ => RegistryResponse {
                        success: false,
                        message: format!("Unknown command: {}", cmd.command),
                        data: None,
                    }
                };
                
                if let Ok(response_json) = serde_json::to_string(&response) {
                    let _ = writer.write_all(response_json.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                }
        } else if let Ok(mcp_msg) = serde_json::from_str::<serde_json::Value>(&line) {
            // Handle MCP-style messages
            let response = if let Some(msg_type) = mcp_msg.get("type").and_then(|t| t.as_str()) {
                match msg_type {
                    "spawn_instances" => {
                        let session_id = mcp_msg.get("session_id").and_then(|s| s.as_str()).unwrap_or("default");
                        let num_instances = mcp_msg.get("num_instances").and_then(|n| n.as_u64()).unwrap_or(2) as u32;
                        let task_description = mcp_msg.get("task_description").and_then(|s| s.as_str()).unwrap_or("Coordination task");
                        
                        let total = registry.increment_instances(session_id, num_instances).await;
                        
                        // Send spawn message to the main Veda process
                        if let Some(veda_pid) = registry.get_session_pid(session_id).await {
                            info!("Routing spawn message to Veda PID {} for session {}", veda_pid, session_id);
                            
                            // If we have app_tx (same process), send directly instead of through socket
                            if let Some(ref app_tx) = app_tx {
                                let spawn_msg = crate::claude::ClaudeMessage::VedaSpawnInstances {
                                    task_description: task_description.to_string(),
                                    num_instances: num_instances as u8,
                                    session_id: session_id.to_string(),
                                };
                                
                                if let Err(e) = app_tx.send(spawn_msg).await {
                                    warn!("Failed to send spawn message directly to main process: {}", e);
                                }
                            } else {
                                // Fallback: send through socket for cross-process communication
                                let routed_msg = json!({
                                    "target_pid": veda_pid,
                                    "message_type": "spawn_instances", 
                                    "task_description": task_description,
                                    "num_instances": num_instances
                                });
                                
                                let _ = writer.write_all(format!("ROUTE_TO_PID:{}\n", serde_json::to_string(&routed_msg).unwrap_or_default()).as_bytes()).await;
                            }
                        }
                        
                        format!("✅ Spawning {} instances for session {}. Total: {}", num_instances, session_id, total)
                    }
                    "list_instances" => {
                        let session_id = mcp_msg.get("session_id").and_then(|s| s.as_str()).unwrap_or("default");
                        let count = registry.get_instances(session_id).await;
                        let all_sessions = registry.get_all_sessions().await;
                        format!("📋 Session '{}' has {} instances. All sessions: {:?}", session_id, count, all_sessions)
                    }
                    "close_instance" => {
                        let session_id = mcp_msg.get("session_id").and_then(|s| s.as_str()).unwrap_or("default");
                        let remaining = registry.decrement_instances(session_id, 1).await;
                        format!("❌ Closed 1 instance for session {}. Remaining: {}", session_id, remaining)
                    }
                    _ => format!("❓ Unknown MCP message type: {}", msg_type)
                }
            } else {
                "❓ Invalid MCP message format".to_string()
            };
            
            let _ = writer.write_all(response.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
        } else {
            warn!("Failed to parse message as either registry command or MCP message: {}", line);
        }
        
        line.clear();
    }
}

/// Client for connecting to the shared registry
pub struct RegistryClient;

impl RegistryClient {
    pub async fn send_command(command: RegistryCommand) -> Result<RegistryResponse> {
        let socket_path = get_socket_path();
        let mut socket = UnixStream::connect(&socket_path).await?;
        
        let command_json = serde_json::to_string(&command)?;
        socket.write_all(command_json.as_bytes()).await?;
        socket.write_all(b"\n").await?;
        
        let (reader, _) = socket.split();
        let mut reader = BufReader::new(reader);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;
        
        let response: RegistryResponse = serde_json::from_str(&response_line)?;
        Ok(response)
    }
    
    pub async fn increment_instances(session_id: &str, count: u32) -> Result<u32> {
        let cmd = RegistryCommand {
            command: "increment".to_string(),
            session_id: session_id.to_string(),
            value: Some(count),
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            // Extract count from message
            if let Some(total_str) = response.message.split(' ').nth(2) {
                if let Ok(total) = total_str.parse::<u32>() {
                    return Ok(total);
                }
            }
        }
        Ok(0)
    }
    
    pub async fn decrement_instances(session_id: &str, count: u32) -> Result<u32> {
        let cmd = RegistryCommand {
            command: "decrement".to_string(),
            session_id: session_id.to_string(),
            value: Some(count),
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            // Extract count from message
            if let Some(remaining_str) = response.message.split(' ').next() {
                if let Ok(remaining) = remaining_str.parse::<u32>() {
                    return Ok(remaining);
                }
            }
        }
        Ok(0)
    }
    
    pub async fn get_instances(session_id: &str) -> Result<u32> {
        let cmd = RegistryCommand {
            command: "get".to_string(),
            session_id: session_id.to_string(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            if let Some(data) = response.data {
                return Ok(data.get(session_id).copied().unwrap_or(0));
            }
        }
        Ok(0)
    }
    
    pub async fn list_all_sessions() -> Result<HashMap<String, u32>> {
        let cmd = RegistryCommand {
            command: "list".to_string(),
            session_id: String::new(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            return Ok(response.data.unwrap_or_default());
        }
        Ok(HashMap::new())
    }
    
    /// Register sessionID -> Veda PID mapping in shared registry
    pub async fn register_session_pid(session_id: &str, veda_pid: u32) -> Result<()> {
        let cmd = RegistryCommand {
            command: "register_pid".to_string(),
            session_id: session_id.to_string(),
            value: Some(veda_pid),
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            info!("✅ Registered session {} -> PID {} in shared registry", session_id, veda_pid);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to register session PID: {}", response.message))
        }
    }
    
    /// Get Veda PID for a session from shared registry
    pub async fn get_session_pid(session_id: &str) -> Result<Option<u32>> {
        let cmd = RegistryCommand {
            command: "get_pid".to_string(),
            session_id: session_id.to_string(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            if let Some(data) = response.data {
                return Ok(data.get(session_id).copied());
            }
        }
        Ok(None)
    }
    
    /// Remove sessionID -> PID mapping from shared registry
    pub async fn unregister_session_pid(session_id: &str) -> Result<()> {
        let cmd = RegistryCommand {
            command: "unregister_pid".to_string(),
            session_id: session_id.to_string(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            info!("✅ Unregistered session {} from shared registry", session_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to unregister session PID: {}", response.message))
        }
    }
    
    /// Get all sessionID -> PID mappings from shared registry
    pub async fn list_all_session_pids() -> Result<HashMap<String, u32>> {
        let cmd = RegistryCommand {
            command: "list_pids".to_string(),
            session_id: String::new(),
            value: None,
        };
        
        let response = Self::send_command(cmd).await?;
        if response.success {
            return Ok(response.data.unwrap_or_default());
        }
        Ok(HashMap::new())
    }
}

/// Connect to the shared registry as a client to listen for routed messages
pub async fn connect_to_registry_as_client(
    app_tx: tokio::sync::mpsc::Sender<crate::claude::ClaudeMessage>,
    veda_pid: u32,
) -> Result<()> {
    let socket_path = get_socket_path();
    
    loop {
        match UnixStream::connect(&socket_path).await {
            Ok(mut stream) => {
                info!("Veda PID {} connected to registry as client", veda_pid);
                
                let (reader, mut writer) = stream.split();
                let mut reader = BufReader::new(reader);
                let mut line = String::new();
                
                // Send registration message
                let register_msg = format!("{{\"command\":\"register_pid\",\"session_id\":\"{}\",\"value\":{}}}\n", 
                    "client", veda_pid);
                let _ = writer.write_all(register_msg.as_bytes()).await;
                
                // Listen for messages from registry
                while reader.read_line(&mut line).await.is_ok() {
                    if line.is_empty() {
                        break;
                    }
                    
                    // Check for routed messages
                    if line.starts_with("ROUTE_TO_PID:") {
                        let json_part = line.trim_start_matches("ROUTE_TO_PID:");
                        if let Ok(routed_msg) = serde_json::from_str::<serde_json::Value>(json_part) {
                            if let Some(target_pid) = routed_msg.get("target_pid").and_then(|p| p.as_u64()) {
                                if target_pid as u32 == veda_pid {
                                    // This message is for us!
                                    info!("Received routed message for PID {}: {:?}", veda_pid, routed_msg);
                                    
                                    if routed_msg.get("message_type").and_then(|t| t.as_str()) == Some("spawn_instances") {
                                        let task_desc = routed_msg.get("task_description").and_then(|t| t.as_str()).unwrap_or("Coordination task");
                                        let num_instances = routed_msg.get("num_instances").and_then(|n| n.as_u64()).unwrap_or(2) as u8;
                                        let session_id = routed_msg.get("session_id").and_then(|s| s.as_str()).unwrap_or("unknown");
                                        
                                        let spawn_msg = crate::claude::ClaudeMessage::VedaSpawnInstances {
                                            task_description: task_desc.to_string(),
                                            num_instances,
                                            session_id: session_id.to_string(),
                                        };
                                        
                                        if let Err(e) = app_tx.send(spawn_msg).await {
                                            warn!("Failed to send spawn message to app: {}", e);
                                        } else {
                                            info!("Successfully routed spawn message to main app");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    line.clear();
                }
                
                warn!("Registry connection lost for PID {}, reconnecting...", veda_pid);
            }
            Err(e) => {
                warn!("Failed to connect to registry as client: {}, retrying in 5 seconds...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }
}

